//! Reports and dashboards (JournalAndArtefactStore.md Internal Logic 12; ADR-067).

use ordin_types::domain::runs::{Dashboard, Report};
use ordin_types::{Cursor, DashboardId, Page, ReportId, RunId, WorkspaceId};

use crate::codec;
use crate::db::{self, Arg, Conditions, DbRow, Tx};
use crate::error::JournalError;
use crate::journal::Journal;
use crate::records::{add_keyset, page_of, page_probe};
use crate::types::{NewDashboard, ReportSummary};
use crate::validate;

/// A dashboard keeps at most this many refresh runs; the oldest are dropped (Internal Logic 12.3).
const MAX_REFRESH_RUNS: usize = 1_000;

const REPORT_COLUMNS: &str = "{s:id}, {s:workspace_id}, {s:run_id}, title, {t:as_of}, {s:parts}";

fn report_from(row: &DbRow) -> Result<Report, JournalError> {
    Ok(Report {
        id: ReportId(codec::uuid(&row.text(0)?)?),
        workspace_id: WorkspaceId(codec::uuid(&row.text(1)?)?),
        run_id: RunId(codec::uuid(&row.text(2)?)?),
        title: row.text(3)?,
        as_of: codec::ts(&row.text(4)?)?,
        parts: codec::from_json(&row.text(5)?)?,
    })
}

const DASHBOARD_COLUMNS: &str =
    "{s:id}, {s:workspace_id}, name, {s:made_by}, {s:inputs}, {s:refresh_runs}, {t:kept_at}";

fn dashboard_from(row: &DbRow) -> Result<Dashboard, JournalError> {
    Ok(Dashboard {
        id: DashboardId(codec::uuid(&row.text(0)?)?),
        workspace_id: WorkspaceId(codec::uuid(&row.text(1)?)?),
        name: row.text(2)?,
        made_by: codec::from_json(&row.text(3)?)?,
        inputs: codec::from_json(&row.text(4)?)?,
        refresh_runs: codec::from_json(&row.text(5)?)?,
        kept_at: codec::ts(&row.text(6)?)?,
    })
}

impl Journal {
    /// The caller builds the 2C `Report`, including its `ReportId`.
    pub async fn store_report(&self, report: Report) -> Result<(), JournalError> {
        let whole = serde_json::to_string(&report).map_err(|_| JournalError::Unavailable)?;
        validate::report(&report.title, &report.parts, whole.len())?;
        let _writer = self.inner.write_lock.lock().await;
        let stored = db::execute(
            &self.inner.store,
            "INSERT INTO reports (id, workspace_id, run_id, title, as_of, parts, recorded_at) \
             VALUES ($1{uuid}, $2{uuid}, $3{uuid}, $4, $5{ts}, $6{json}, $7{ts})",
            &[
                Arg::Text(report.id.to_string()),
                Arg::Text(report.workspace_id.to_string()),
                Arg::Text(report.run_id.to_string()),
                Arg::Text(report.title.clone()),
                Arg::Text(report.as_of.to_wire()),
                Arg::Text(codec::to_json(&report.parts)?),
                Arg::Text(self.now().to_wire()),
            ],
        )
        .await;
        match stored {
            Ok(_) => Ok(()),
            // SPEC-GAP-6: `reports.run_id` is unique (Data Model) but no code is named for a second
            // report of one run, or a reused `ReportId`; the call is refused as an invalid record.
            Err(e) if db::is_unique_violation(&e) => Err(JournalError::EntryInvalid),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn report(&self, id: ReportId) -> Result<Report, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {REPORT_COLUMNS} FROM reports WHERE id = $1{{uuid}}"),
            &[Arg::Text(id.to_string())],
        )
        .await?
        .map(|row| report_from(&row))
        .transpose()?
        .ok_or(JournalError::NotFound)
    }

    pub async fn report_for_run(&self, run_id: RunId) -> Result<Option<Report>, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {REPORT_COLUMNS} FROM reports WHERE run_id = $1{{uuid}}"),
            &[Arg::Text(run_id.to_string())],
        )
        .await?
        .map(|row| report_from(&row))
        .transpose()
    }

    /// Newest first by `(as_of DESC, id DESC)`, the order of index `reports_workspace`.
    pub async fn list_reports(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<Page<ReportSummary>, JournalError> {
        let mut conditions = Conditions::default();
        conditions.add(
            "workspace_id = #1{uuid}",
            vec![Arg::Text(workspace_id.to_string())],
        );
        add_keyset(&mut conditions, cursor.as_ref(), "as_of", "id", "{uuid}")?;
        let sql = format!(
            "SELECT {{s:id}}, {{s:run_id}}, title, {{t:as_of}} FROM reports{} \
             ORDER BY as_of DESC, id DESC LIMIT ${}",
            conditions.sql(),
            conditions.next()
        );
        let mut args = conditions.args;
        args.push(Arg::Int(page_probe()));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        let items = rows
            .iter()
            .map(|row| {
                Ok(ReportSummary {
                    id: ReportId(codec::uuid(&row.text(0)?)?),
                    run_id: RunId(codec::uuid(&row.text(1)?)?),
                    title: row.text(2)?,
                    as_of: codec::ts(&row.text(3)?)?,
                })
            })
            .collect::<Result<Vec<_>, JournalError>>()?;
        Ok(page_of(items, |r| (r.as_of, r.id.0)))
    }

    pub async fn keep_dashboard(&self, new: NewDashboard) -> Result<Dashboard, JournalError> {
        ordin_types::text::check_len(&new.name, 1, 80, "REPORT_DASHBOARD_NAME_DUPLICATE")?;
        validate::inputs(&new.inputs)?;
        if self.report_for_run(new.run_id).await?.is_none() {
            return Err(JournalError::NotFound);
        }
        let dashboard = Dashboard {
            id: DashboardId(uuid::Uuid::now_v7()),
            workspace_id: new.workspace_id,
            name: new.name,
            made_by: new.made_by,
            inputs: new.inputs,
            refresh_runs: vec![new.run_id],
            kept_at: self.now(),
        };
        let _writer = self.inner.write_lock.lock().await;
        let kept = db::execute(
            &self.inner.store,
            "INSERT INTO dashboards (id, workspace_id, name, made_by, inputs, refresh_runs, kept_at) \
             VALUES ($1{uuid}, $2{uuid}, $3, $4{json}, $5{json}, $6{json}, $7{ts})",
            &[
                Arg::Text(dashboard.id.to_string()),
                Arg::Text(dashboard.workspace_id.to_string()),
                Arg::Text(dashboard.name.clone()),
                Arg::Text(codec::to_json(&dashboard.made_by)?),
                Arg::Text(codec::to_json(&dashboard.inputs)?),
                Arg::Text(codec::to_json(&dashboard.refresh_runs)?),
                Arg::Text(dashboard.kept_at.to_wire()),
            ],
        )
        .await;
        match kept {
            Ok(_) => Ok(dashboard),
            Err(e) if db::is_unique_violation(&e) => {
                Err(JournalError::invalid("REPORT_DASHBOARD_NAME_DUPLICATE"))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn add_dashboard_refresh(
        &self,
        id: DashboardId,
        run_id: RunId,
    ) -> Result<Dashboard, JournalError> {
        if self.report_for_run(run_id).await?.is_none() {
            return Err(JournalError::NotFound);
        }
        let _writer = self.inner.write_lock.lock().await;
        let mut dashboard = self.dashboard(id).await?.ok_or(JournalError::NotFound)?;
        dashboard.refresh_runs.push(run_id);
        if dashboard.refresh_runs.len() > MAX_REFRESH_RUNS {
            let surplus = dashboard.refresh_runs.len() - MAX_REFRESH_RUNS;
            dashboard.refresh_runs.drain(..surplus);
        }
        db::execute(
            &self.inner.store,
            "UPDATE dashboards SET refresh_runs = $1{json} WHERE id = $2{uuid}",
            &[
                Arg::Text(codec::to_json(&dashboard.refresh_runs)?),
                Arg::Text(id.to_string()),
            ],
        )
        .await?;
        Ok(dashboard)
    }

    pub async fn remove_dashboard(&self, id: DashboardId) -> Result<(), JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let removed = db::execute(
            &self.inner.store,
            "DELETE FROM dashboards WHERE id = $1{uuid}",
            &[Arg::Text(id.to_string())],
        )
        .await?;
        if removed == 0 {
            return Err(JournalError::NotFound);
        }
        Ok(())
    }

    /// Newest first by `(kept_at DESC, id DESC)` *(order chosen here; Pagination.md lists none)*.
    pub async fn list_dashboards(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<Page<Dashboard>, JournalError> {
        let mut conditions = Conditions::default();
        conditions.add(
            "workspace_id = #1{uuid}",
            vec![Arg::Text(workspace_id.to_string())],
        );
        add_keyset(&mut conditions, cursor.as_ref(), "kept_at", "id", "{uuid}")?;
        let sql = format!(
            "SELECT {DASHBOARD_COLUMNS} FROM dashboards{} ORDER BY kept_at DESC, id DESC LIMIT ${}",
            conditions.sql(),
            conditions.next()
        );
        let mut args = conditions.args;
        args.push(Arg::Int(page_probe()));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        let items = rows
            .iter()
            .map(dashboard_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(page_of(items, |d: &Dashboard| (d.kept_at, d.id.0)))
    }

    /// One dashboard by id; `Ok(None)` when absent (SA-CLOSE-01, K6).
    // spec: ordin-desktop/components/JournalAndArtefactStore#SA-CLOSE-01
    pub async fn dashboard(&self, id: DashboardId) -> Result<Option<Dashboard>, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {DASHBOARD_COLUMNS} FROM dashboards WHERE id = $1{{uuid}}"),
            &[Arg::Text(id.to_string())],
        )
        .await?
        .map(|row| dashboard_from(&row))
        .transpose()
    }

    /// Removes reports and dashboards of a removed workspace (04 Data Lifecycle).
    pub async fn forget_workspace(&self, workspace_id: WorkspaceId) -> Result<u64, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let args = [Arg::Text(workspace_id.to_string())];
        let dashboards = tx
            .execute(
                "DELETE FROM dashboards WHERE workspace_id = $1{uuid}",
                &args,
            )
            .await?;
        let reports = tx
            .execute("DELETE FROM reports WHERE workspace_id = $1{uuid}", &args)
            .await?;
        tx.commit().await?;
        Ok(dashboards + reports)
    }

    /// The last run of every dashboard's `refresh_runs`: their reports are never pruned
    /// (Internal Logic 14.3.4).
    pub(crate) async fn shown_report_runs(&self) -> Result<Vec<String>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            "SELECT {s:refresh_runs} FROM dashboards",
            &[],
        )
        .await?;
        let mut runs = Vec::with_capacity(rows.len());
        for row in &rows {
            let refresh: Vec<RunId> = codec::from_json(&row.text(0)?)?;
            if let Some(last) = refresh.last() {
                runs.push(last.to_string());
            }
        }
        runs.sort();
        runs.dedup();
        Ok(runs)
    }

    /// Removes `run_ids` from every dashboard's `refresh_runs` (Internal Logic 14.4).
    pub(crate) async fn drop_refresh_runs(
        &self,
        tx: &mut Tx,
        run_ids: &[RunId],
    ) -> Result<(), JournalError> {
        let rows = tx
            .fetch_all("SELECT {s:id}, {s:refresh_runs} FROM dashboards", &[])
            .await?;
        for row in &rows {
            let refresh: Vec<RunId> = codec::from_json(&row.text(1)?)?;
            let kept: Vec<RunId> = refresh
                .iter()
                .copied()
                .filter(|r| !run_ids.contains(r))
                .collect();
            if kept.len() != refresh.len() {
                tx.execute(
                    "UPDATE dashboards SET refresh_runs = $1{json} WHERE id = $2{uuid}",
                    &[Arg::Text(codec::to_json(&kept)?), Arg::Text(row.text(0)?)],
                )
                .await?;
            }
        }
        Ok(())
    }
}
