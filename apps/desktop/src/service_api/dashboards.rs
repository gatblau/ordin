//! `dashboards.export` and `dashboards.add_to_presentation` (OrdinDesktop.md DSK-DBX-1 to DSK-DBX-4;
//! ServiceApi.md SVC-DBX-1; amended January 2026; user decision; Approved by the user January 2026):
//! the dashboard's newest report, read from the Journal, written by [`crate::dashboard_export`] on
//! this computer. Neither the report nor the file's bytes are logged.

use std::path::{Path, PathBuf};

use ordin_localisation::{ArgValue, DateTimeStyle, MessageArgs};
use ordin_service_api::commands::dashboards::{
    DashboardsAddToPresentationInput, DashboardsExportInput, PresentationUpdated,
};
use ordin_service_api::commands::SavedCopy;
use ordin_service_api::ServiceError;
use ordin_types::domain::{Dashboard, Report};
use ordin_types::DashboardId;

use super::DesktopServiceApi;
use crate::dashboard_export::{self, Words};
use crate::error::{
    DASHBOARD_NOT_FOUND, DASHBOARD_NO_REPORT, DIALOG_CANCELLED, PICKER_UNAVAILABLE,
    PRESENTATION_UNREADABLE, SAVE_COPY_FAILED,
};

/// The largest presentation read to add slides to (DSK-DBX-2).
const MAX_PRESENTATION_FILE_BYTES: u64 = 1 << 30;

impl DesktopServiceApi {
    /// The dashboard and the report of the last of its `refresh_runs` that has one (as the
    /// dashboard's screen shows, ScreensModule.md SM-DB-4).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
    async fn newest_dashboard_report(
        &self,
        dashboard_id: DashboardId,
    ) -> Result<(Dashboard, Report), ServiceError> {
        let journal = self.handle(&self.modules.journal)?;
        let dashboard = journal
            .dashboard(dashboard_id)
            .await
            .map_err(|e| self.module_error(e))?
            .ok_or_else(|| self.own_refusal(DASHBOARD_NOT_FOUND, false))?;
        for run_id in dashboard.refresh_runs.iter().rev() {
            if let Some(report) = journal
                .report_for_run(*run_id)
                .await
                .map_err(|e| self.module_error(e))?
            {
                return Ok((dashboard, report));
            }
        }
        Err(self.own_refusal(DASHBOARD_NO_REPORT, false))
    }

    /// The words the files hold, in the active locale; en-GB without Localisation.
    fn export_words(&self, report: &Report) -> Words {
        let Some(localisation) = &self.modules.localisation else {
            return Words::english(format!(
                "As of {}",
                report.as_of.0.format("%Y-%m-%d %H:%M UTC")
            ));
        };
        let time = localisation.format_datetime(report.as_of, DateTimeStyle::DateTime);
        let message = |id| localisation.message(id, &MessageArgs::new());
        Words {
            as_of: plain(&localisation.message(
                ordin_localisation::id::DASHBOARD_AS_OF,
                &MessageArgs::new().with("time", ArgValue::Text(time)),
            )),
            figure: message(ordin_localisation::id::DASHBOARD_EXPORT_FIGURE),
            value: message(ordin_localisation::id::DASHBOARD_EXPORT_VALUE),
            unit: message(ordin_localisation::id::DASHBOARD_EXPORT_UNIT),
        }
    }

    /// `dashboards.export` (DSK-DBX-1): the newest report as the chosen format, then the save
    /// dialog with the dashboard's name proposed and an atomic write, as `files.save_export` does.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
    pub(super) async fn dashboards_export(
        &self,
        input: DashboardsExportInput,
    ) -> Result<SavedCopy, ServiceError> {
        let (dashboard, report) = self.newest_dashboard_report(input.dashboard_id).await?;
        let words = self.export_words(&report);
        let bytes = dashboard_export::render(&dashboard.name, &report, &words, input.format);
        let shell = self.handle(&self.modules.shell)?;
        let path = shell
            .save_path(&dashboard_export::file_name(&dashboard.name, input.format))
            .await
            .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
        crate::shell::write_atomically(&path, &bytes)
            .map_err(|_| self.own_refusal(SAVE_COPY_FAILED, true))?;
        Ok(SavedCopy {
            saved_path: path.to_string_lossy().into_owned(),
        })
    }

    /// `dashboards.add_to_presentation` (DSK-DBX-2, DSK-DBX-3): the person picks a presentation;
    /// the new slides are added after its last one and the file is replaced atomically, so it is
    /// either as it was or holds every new slide. When the presentation is open in another program,
    /// or cannot be replaced, the save dialog offers a copy instead and the original is untouched.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-2
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-3
    pub(super) async fn dashboards_add_to_presentation(
        &self,
        input: DashboardsAddToPresentationInput,
    ) -> Result<PresentationUpdated, ServiceError> {
        let (dashboard, report) = self.newest_dashboard_report(input.dashboard_id).await?;
        let shell = self.handle(&self.modules.shell)?;
        let title = match &self.modules.localisation {
            Some(localisation) => localisation.message(
                ordin_localisation::id::DASHBOARD_EXPORT_PICK_PRESENTATION,
                &MessageArgs::new(),
            ),
            None => "Choose a PowerPoint presentation".to_owned(),
        };
        let path = shell
            .pick_file(&title)
            .await
            .map_err(|_| self.own_refusal(PICKER_UNAVAILABLE, true))?
            .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
        let original = read_presentation(&path)
            .ok_or_else(|| self.own_refusal(PRESENTATION_UNREADABLE, false))?;
        let words = self.export_words(&report);
        let bytes = dashboard_export::add_slides(&original, &dashboard.name, &report, &words)
            .map_err(|_| self.own_refusal(PRESENTATION_UNREADABLE, false))?;
        if !is_open_elsewhere(&path) && crate::shell::write_atomically(&path, &bytes).is_ok() {
            return Ok(PresentationUpdated {
                saved_path: path.to_string_lossy().into_owned(),
                as_copy: false,
            });
        }
        tracing::info!(
            component = "desktop",
            command = "dashboards.add_to_presentation",
            "presentation_saved_as_copy"
        );
        let (presentation, dashboard_stem) = copy_stems(&path, &dashboard.name);
        let copy_name = match &self.modules.localisation {
            Some(localisation) => format!(
                "{}.pptx",
                plain(
                    &localisation.message(
                        ordin_localisation::id::DASHBOARD_EXPORT_COPY_NAME,
                        &MessageArgs::new()
                            .with("presentation", ArgValue::Text(presentation))
                            .with("dashboard", ArgValue::Text(dashboard_stem)),
                    )
                )
            ),
            None => format!("{presentation} with {dashboard_stem}.pptx"),
        };
        let copy = shell
            .save_path(&copy_name)
            .await
            .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
        crate::shell::write_atomically(&copy, &bytes)
            .map_err(|_| self.own_refusal(SAVE_COPY_FAILED, true))?;
        Ok(PresentationUpdated {
            saved_path: copy.to_string_lossy().into_owned(),
            as_copy: true,
        })
    }
}

/// A message without the direction marks Fluent puts around each argument: a file holds the text
/// itself, and a file name must not carry them.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{2068}' | '\u{2069}'))
        .collect()
}

/// The picked file's bytes; `None` when it cannot be read or is over 1 GiB.
fn read_presentation(path: &Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_PRESENTATION_FILE_BYTES {
        return None;
    }
    std::fs::read(path).ok()
}

/// True when PowerPoint has the file open: it keeps a `~$<name>` owner file beside it while it does
/// (DSK-DBX-3).
// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-3
pub(crate) fn is_open_elsewhere(path: &Path) -> bool {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    folder
        .join(format!("~${}", name.to_string_lossy()))
        .exists()
}

/// The two names the copy's proposed name is made of (`dashboard-export-copy-name`): the
/// presentation's, without its extension, and the dashboard's, each safe in a file name.
pub(crate) fn copy_stems(path: &Path, dashboard: &str) -> (String, String) {
    let stem = path
        .file_stem()
        .map_or_else(|| PathBuf::from("Presentation"), PathBuf::from);
    (
        dashboard_export::safe_stem(&stem.to_string_lossy()),
        dashboard_export::safe_stem(dashboard),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-3
    #[test]
    fn a_presentation_open_in_powerpoint_is_not_replaced() {
        let folder = std::env::temp_dir().join(format!("ordin-dbx-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&folder).unwrap();
        let deck = folder.join("Board pack.pptx");
        std::fs::write(&deck, b"x").unwrap();
        assert!(!is_open_elsewhere(&deck));
        std::fs::write(folder.join("~$Board pack.pptx"), b"").unwrap();
        assert!(is_open_elsewhere(&deck));
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(
            copy_stems(&deck, "Q3: prices"),
            ("Board pack".to_owned(), "Q3  prices".to_owned())
        );
    }
}
