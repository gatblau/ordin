//! GitHub and GitLab approval settings (ADR-023; Internal Logic 8.1, 8.2).

use ordin_rules::GitHostSetting;
use ordin_types::domain::{GithubOperationSetting, GithubSettingSource};
use ordin_types::time::Timestamp;
use ordin_types::{GithubOperationSettingId, ProjectId};
use serde_json::json;

use crate::error::ProjectError;
use crate::service::ProjectService;
use crate::store;

impl ProjectService {
    /// Steps 9.1 and 8.1 with the current `ProjectSettings`.
    pub(crate) async fn reconcile_settings(&self) -> Result<(), ProjectError> {
        self.reconcile_languages().await?;
        for row in store::open_projects(&self.pool).await? {
            self.ensure_git_host_settings(row.project.id).await?;
        }
        Ok(())
    }

    /// Internal Logic 8.1: one row per kind; a kind in the ceiling is forced. A row nobody chose
    /// reads `engineer`: `organisation_profile` marks only what the ceiling forces.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-03
    pub(crate) async fn ensure_git_host_settings(
        &self,
        project: ProjectId,
    ) -> Result<(), ProjectError> {
        let row = store::project(&self.pool, project)
            .await?
            .ok_or(ProjectError::NotFound)?;
        let ceiling = self.current_settings().always_approved_kinds;
        let existing = store::settings_of(&self.pool, project).await?;
        for kind in crate::OPERATION_KINDS {
            let forced = ceiling.iter().any(|c| c == kind);
            match existing.iter().find(|s| s.operation_kind == kind) {
                Some(found) => {
                    if forced
                        && (!found.approval_required
                            || found.set_by != GithubSettingSource::OrganisationProfile)
                    {
                        let mut changed = found.clone();
                        changed.approval_required = true;
                        changed.set_by = GithubSettingSource::OrganisationProfile;
                        changed.set_at = Timestamp::now();
                        store::update_setting(&self.pool, &changed).await?;
                    }
                }
                None => {
                    let is_write = kind != "read";
                    let setting = GithubOperationSetting {
                        id: GithubOperationSettingId::new(),
                        project_id: project,
                        operation_kind: kind.to_owned(),
                        is_write,
                        approval_required: forced || is_write,
                        set_by: if forced {
                            GithubSettingSource::OrganisationProfile
                        } else {
                            GithubSettingSource::Engineer
                        },
                        set_at: Timestamp::now(),
                    };
                    store::insert_setting(&self.pool, &setting).await?;
                }
            }
        }
        self.push_git_host_settings(project, row.project.place_id)
            .await
    }

    async fn push_git_host_settings(
        &self,
        project: ProjectId,
        place: ordin_types::PlaceId,
    ) -> Result<(), ProjectError> {
        let rows = store::settings_of(&self.pool, project).await?;
        self.rules.replace_git_host_settings(
            place,
            rows.into_iter()
                .map(|row| GitHostSetting {
                    kind: row.operation_kind,
                    approval_required: row.approval_required,
                })
                .collect(),
        );
        Ok(())
    }

    pub async fn git_host_settings(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<GithubOperationSetting>, ProjectError> {
        self.open_row(project_id).await?;
        store::settings_of(&self.pool, project_id).await
    }

    /// Internal Logic 8.2.
    // spec: ordin-desktop/components/ProjectService#ADR-023
    // spec: ordin-desktop/components/ProjectService#SA-GH-01
    pub async fn set_git_host_approval(
        &self,
        project_id: ProjectId,
        kind: &str,
        approval_required: bool,
    ) -> Result<GithubOperationSetting, ProjectError> {
        let row = self.open_row(project_id).await?;
        let settings = store::settings_of(&self.pool, project_id).await?;
        let found = settings
            .into_iter()
            .find(|s| s.operation_kind == kind)
            .ok_or(ProjectError::NotFound)?;
        let ceiling = self.current_settings().always_approved_kinds;
        if !approval_required && ceiling.iter().any(|c| c == kind) {
            return Err(ProjectError::Invalid("GHOP_ABOVE_CEILING"));
        }
        // "`read` cannot be set to require approval" names no error code; the row stays
        // as it is and is returned.
        if kind == "read" && approval_required {
            return Ok(found);
        }
        let mut changed = found;
        changed.approval_required = approval_required;
        changed.set_by = GithubSettingSource::Engineer;
        changed.set_at = Timestamp::now();
        store::update_setting(&self.pool, &changed).await?;
        self.push_git_host_settings(project_id, row.project.place_id)
            .await?;
        self.record(
            "git_host_approval_set",
            None,
            Some(row.workspace_id),
            "The engineer changed an approval setting.",
            json!({
                "project_id": project_id.to_string(),
                "kind": kind,
                "approval_required": approval_required,
            }),
        )
        .await;
        Ok(changed)
    }
}
