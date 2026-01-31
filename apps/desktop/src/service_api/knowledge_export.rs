//! `knowledge.export` (OrdinDesktop.md DSK-KEX-1, DSK-KEX-2; ServiceApi.md SVC-KEX-1; amended
//! January 2026; user decision; Approved by the user January 2026): a knowledge page, read by its id
//! through the Knowledge Service as `knowledge.get` reads it, written as a PDF or Word document by
//! [`crate::dashboard_export::render_page`] on this computer. Neither the page nor the file's bytes
//! are logged.

use ordin_service_api::commands::knowledge::{KnowledgeExportFormat, KnowledgeExportInput};
use ordin_service_api::commands::SavedCopy;
use ordin_service_api::ServiceError;

use super::DesktopServiceApi;
use crate::dashboard_export;
use crate::error::{DIALOG_CANCELLED, SAVE_COPY_FAILED};

impl DesktopServiceApi {
    /// `knowledge.export` (DSK-KEX-1): the page's title and Markdown from the Knowledge Service (a
    /// page it does not hold → its own `KNOW_PAGE_NOT_FOUND`), the file written in memory, then the
    /// save dialog with the page's title proposed and an atomic write, as `dashboards.export` does.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-KEX-1
    pub(super) async fn knowledge_export(
        &self,
        input: KnowledgeExportInput,
    ) -> Result<SavedCopy, ServiceError> {
        let knowledge = self.handle(&self.modules.knowledge)?;
        let page_id = self.restate(input.page_id)?;
        let view = knowledge
            .get_page(page_id)
            .await
            .map_err(|e| self.module_error(e))?;
        self.export_page(&view.page.title, &view.page.body_markdown, input.format)
            .await
    }

    /// The one writer of `knowledge.export` and `minutes.export` (DSK-KEX-1, DSK-MIN-4): `title` and
    /// the Markdown `body` written in memory as `format`, then the save dialog with
    /// `<title>.<pdf|docx>` proposed and an atomic write. Cancelled → `DSK_DIALOG_CANCELLED`; a write
    /// failure → `DSK_SAVE_COPY_FAILED`. A fix here is a fix to both rows.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-4
    pub(super) async fn export_page(
        &self,
        title: &str,
        body: &str,
        format: KnowledgeExportFormat,
    ) -> Result<SavedCopy, ServiceError> {
        let bytes = dashboard_export::render_page(title, body, format);
        let shell = self.handle(&self.modules.shell)?;
        let path = shell
            .save_path(&dashboard_export::page_file_name(title, format))
            .await
            .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
        crate::shell::write_atomically(&path, &bytes)
            .map_err(|_| self.own_refusal(SAVE_COPY_FAILED, true))?;
        Ok(SavedCopy {
            saved_path: path.to_string_lossy().into_owned(),
        })
    }
}
