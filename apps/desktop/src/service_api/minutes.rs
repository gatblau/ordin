//! The `minutes.*` commands (ServiceApi.md SVC-MIN-1 to SVC-MIN-4; OrdinDesktop.md DSK-MIN-1 to
//! DSK-MIN-5) over the Minutes Service (row 3.41a), the Journal, the Identity Service and the shared
//! export writer. Compiled only with the `minutes` switch; a build without it has no `minutes` row, so
//! none of these methods is reached.
//!
//! Before `minutes.status`, `minutes.import` and `minutes.record` the program asks the Workspace
//! Service whether the workspace exists and is active, and answers `MIN_WORKSPACE_UNKNOWN` when it is
//! not: the Minutes Service cannot ask (MinutesService.md B1; plan 07 FU-011).
//!
//! The person-only commands (`record`, `give_model_consent`, `accept_notice`, `save_obs_password`,
//! `remove`, SVC-MIN-2) reach the service from this module only. The OBS password is never logged.

#[cfg(not(feature = "minutes"))]
use super::DesktopServiceApi;
#[cfg(not(feature = "minutes"))]
use ordin_types::WorkspaceId;

#[cfg(feature = "minutes")]
mod built {
    use std::sync::Arc;

    use ordin_identity::SecretString;
    use ordin_minutes::MinutesService;
    use ordin_service_api::commands::minutes as api;
    use ordin_service_api::commands::SavedCopy;
    use ordin_service_api::{Done, ServiceError};
    use ordin_types::{Page, WorkspaceId};

    use crate::error::{DIALOG_CANCELLED, PICKER_UNAVAILABLE};
    use crate::service_api::DesktopServiceApi;

    /// The Minutes Service's code for a workspace it cannot use (MinutesService.md Error Table).
    const WORKSPACE_UNKNOWN: &str = "MIN_WORKSPACE_UNKNOWN";
    /// The Minutes Service's code for minutes absent from the Journal.
    const NOT_FOUND: &str = "MIN_NOT_FOUND";
    /// The file kinds `minutes.import` offers (DSK-MIN-2).
    const IMPORT_EXTENSIONS: &[&str] = &["vtt", "docx", "txt", "m4a", "mp3", "wav"];

    impl DesktopServiceApi {
        fn minutes(&self) -> Result<Arc<MinutesService>, ServiceError> {
            self.handle(&self.modules.minutes)
        }

        /// FU-011: the workspace exists and is active, else `MIN_WORKSPACE_UNKNOWN`.
        // spec: ordin-desktop/components/MinutesService#MIN_WORKSPACE_UNKNOWN
        async fn minutes_workspace(&self, workspace_id: WorkspaceId) -> Result<(), ServiceError> {
            let workspaces = self.handle(&self.modules.workspaces)?;
            workspaces
                .ensure_active(workspace_id)
                .await
                .map(|_| ())
                .map_err(|_| self.refuse(WORKSPACE_UNKNOWN, None))
        }

        /// The minutes of `minutes_id` from the Journal; absent → `MIN_NOT_FOUND`.
        async fn kept_minutes(
            &self,
            minutes_id: ordin_types::MinutesId,
        ) -> Result<api::Minutes, ServiceError> {
            let journal = self.handle(&self.modules.journal)?;
            journal
                .minutes(minutes_id)
                .await
                .map_err(|e| self.module_error(e))?
                .ok_or_else(|| self.refuse(NOT_FOUND, None))
        }

        // spec: ordin-desktop/components/ServiceApi#SVC-MIN-1
        pub(in crate::service_api) async fn min_status(
            &self,
            input: api::MinutesWorkspaceInput,
        ) -> Result<api::MinutesStatus, ServiceError> {
            self.minutes_workspace(input.workspace_id).await?;
            self.minutes()?
                .status(input.workspace_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_list(
            &self,
            input: api::MinutesListInput,
        ) -> Result<Page<api::MinutesSummary>, ServiceError> {
            let journal = self.handle(&self.modules.journal)?;
            journal
                .list_minutes(input.workspace_id, input.cursor)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_get(
            &self,
            input: api::MinutesIdInput,
        ) -> Result<api::Minutes, ServiceError> {
            self.kept_minutes(input.minutes_id).await
        }

        pub(in crate::service_api) async fn min_transcript(
            &self,
            input: api::MinutesIdInput,
        ) -> Result<api::Transcript, ServiceError> {
            self.minutes()?
                .transcript(input.minutes_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_save_edit(
            &self,
            input: api::MinutesSaveEditInput,
        ) -> Result<api::Minutes, ServiceError> {
            self.minutes()?
                .save_edit(input.minutes_id, input.body, input.base_revision)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_remove(
            &self,
            input: api::MinutesIdInput,
        ) -> Result<Done, ServiceError> {
            self.minutes()?
                .remove(input.minutes_id)
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        /// DSK-MIN-2: the file dialog, then `import`; the file is read where it is.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-2
        pub(in crate::service_api) async fn min_import(
            &self,
            input: api::MinutesWorkspaceInput,
        ) -> Result<api::MinutesJob, ServiceError> {
            self.minutes_workspace(input.workspace_id).await?;
            let minutes = self.minutes()?;
            let shell = self.handle(&self.modules.shell)?;
            let (title, filter) = match &self.modules.localisation {
                Some(localisation) => (
                    localisation.message(
                        ordin_localisation::id::MINUTES_IMPORT_TITLE,
                        &ordin_localisation::MessageArgs::new(),
                    ),
                    localisation.message(
                        ordin_localisation::id::MINUTES_IMPORT_FILTER,
                        &ordin_localisation::MessageArgs::new(),
                    ),
                ),
                None => (
                    "Choose a meeting transcript or recording".to_owned(),
                    "Transcripts and recordings".to_owned(),
                ),
            };
            let path = shell
                .pick_file_of(&title, &filter, IMPORT_EXTENSIONS)
                .await
                .map_err(|_| self.own_refusal(PICKER_UNAVAILABLE, true))?
                .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
            minutes
                .import(input.workspace_id, path)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_jobs(
            &self,
            input: api::MinutesWorkspaceInput,
        ) -> Result<Vec<api::MinutesJob>, ServiceError> {
            self.minutes()?
                .jobs(input.workspace_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_cancel_job(
            &self,
            input: api::MinutesJobInput,
        ) -> Result<api::MinutesJob, ServiceError> {
            self.minutes()?
                .cancel_job(input.job_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_retry(
            &self,
            input: api::MinutesJobInput,
        ) -> Result<api::MinutesJob, ServiceError> {
            self.minutes()?
                .retry(input.job_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_give_model_consent(
            &self,
        ) -> Result<Done, ServiceError> {
            self.minutes()?
                .give_model_consent()
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        pub(in crate::service_api) async fn min_remove_model(&self) -> Result<Done, ServiceError> {
            self.minutes()?
                .remove_model()
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        pub(in crate::service_api) async fn min_accept_notice(&self) -> Result<Done, ServiceError> {
            self.minutes()?
                .accept_writer_notice()
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        pub(in crate::service_api) async fn min_change_settings(
            &self,
            input: api::MinutesSettingsChange,
        ) -> Result<api::MinutesStatus, ServiceError> {
            self.minutes()?
                .change_settings(input)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_record(
            &self,
            input: api::MinutesWorkspaceInput,
        ) -> Result<api::RecordingView, ServiceError> {
            self.minutes_workspace(input.workspace_id).await?;
            self.minutes()?
                .record(input.workspace_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_stop(
            &self,
            input: api::MinutesStopInput,
        ) -> Result<api::MinutesJob, ServiceError> {
            self.minutes()?
                .stop(input.recording_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        pub(in crate::service_api) async fn min_open_obs(&self) -> Result<Done, ServiceError> {
            self.minutes()?
                .open_obs()
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        /// DSK-MIN-3: a new password replaces the stored one; never logged or echoed.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-3
        pub(in crate::service_api) async fn min_save_obs_password(
            &self,
            input: api::ObsPasswordInput,
        ) -> Result<Done, ServiceError> {
            let identity = self.handle(&self.modules.identity)?;
            identity
                .save_obs_password(SecretString::new(input.password))
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        pub(in crate::service_api) async fn min_delete_recording(
            &self,
            input: api::MinutesIdInput,
        ) -> Result<api::Minutes, ServiceError> {
            self.minutes()?
                .delete_recording(input.minutes_id)
                .await
                .map_err(|e| self.module_error(e))
        }

        /// DSK-MIN-4: the minutes from the Journal, written by the writer `knowledge.export` uses.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-4
        pub(in crate::service_api) async fn min_export(
            &self,
            input: api::MinutesExportInput,
        ) -> Result<SavedCopy, ServiceError> {
            let minutes = self.kept_minutes(input.minutes_id).await?;
            self.export_page(&minutes.title, &minutes.body, input.format)
                .await
        }

        /// DSK-MIN-1: after a workspace is deleted, its minutes, jobs and recordings go too. A
        /// failure is logged with its code only; the deletion itself has succeeded.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
        pub(in crate::service_api) async fn minutes_workspace_removed(&self, id: WorkspaceId) {
            let Some(minutes) = self.modules.minutes.clone() else {
                return;
            };
            if let Err(e) = minutes.workspace_removed(id).await {
                tracing::warn!(
                    component = "desktop",
                    code = ordin_types::CodedError::code(&e),
                    "minutes.workspace_removed_failed"
                );
            }
        }
    }
}

#[cfg(not(feature = "minutes"))]
impl DesktopServiceApi {
    /// A build without `minutes` keeps no minutes.
    pub(in crate::service_api) async fn minutes_workspace_removed(&self, _id: WorkspaceId) {}
}
