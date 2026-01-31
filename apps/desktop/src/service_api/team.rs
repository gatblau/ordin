//! The `team_knowledge.*` commands over the Team Knowledge Service (OrdinDesktop.md DSK-TW-1 and
//! DSK-TW-3). Its first build answers the seven commands of ADR-103 and the eleven of a team workspace
//! (ServiceApi.md SVC-TW-1); the seven commands of reading other teams' folders (ADR-110) are not
//! answered (`SVC_NOT_IN_THIS_VERSION`, DSK-TW-1). A build without `team-knowledge` answers none.
//!
//! The person-only commands (`open_workspace`, `allow_grant`, `decline_grant`, SVC-TW-2) reach the
//! service from this module only, which the Architecture Test allows.

use ordin_service_api::commands::team_knowledge as api;
use ordin_service_api::{Done, ServiceError};

use super::DesktopServiceApi;

#[cfg(feature = "team-knowledge")]
mod built {
    use std::sync::Arc;

    use ordin_team_knowledge::TeamKnowledgeService;

    use super::*;

    impl DesktopServiceApi {
        fn team(&self) -> Result<Arc<TeamKnowledgeService>, ServiceError> {
            self.handle(&self.modules.team_knowledge)
        }

        /// The owner's status in the Service API's form: the read-folder fields of ADR-110, which this
        /// build does not have, are empty (DSK-TW-1).
        fn team_status(
            &self,
            status: ordin_team_knowledge::TeamKnowledgeStatus,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let mut value = self.value(status)?;
            if let Some(map) = value.as_object_mut() {
                let own = map
                    .get("binding")
                    .and_then(|b| b.get("id"))
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                map.insert("own_folder_id".to_owned(), own);
                map.insert("folders".to_owned(), serde_json::json!([]));
                map.insert("workspace_facts".to_owned(), serde_json::json!(0));
                map.insert("workspace_large".to_owned(), serde_json::json!(false));
                map.insert("last_recall_check".to_owned(), serde_json::Value::Null);
                map.insert("descriptions".to_owned(), serde_json::json!([]));
            }
            self.restate(value)
        }

        /// A `folder_id` names a read folder, which this build has none of.
        fn own_folder_only(
            &self,
            folder: Option<ordin_types::TeamFolderId>,
            ws: ordin_types::WorkspaceId,
        ) -> Result<(), ServiceError> {
            match folder {
                Some(f) if f.0 != ws.0 => Err(self.refuse("TEAM_FOLDER_NOT_FOUND", None)),
                _ => Ok(()),
            }
        }

        pub(crate) async fn tk_status(
            &self,
            i: api::TeamKnowledgeStatusInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let s = self
                .team()?
                .status(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_candidate_links(
            &self,
            i: api::TeamKnowledgeCandidateLinksInput,
        ) -> Result<Vec<api::SourceLink>, ServiceError> {
            let links = self
                .team()?
                .candidate_links(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.restate(links)
        }

        pub(crate) async fn tk_turn_on(
            &self,
            i: api::TeamKnowledgeTurnOnInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let choice = self.restate(i.choice)?;
            let s = self
                .team()?
                .turn_on(i.ws, choice)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_turn_off(
            &self,
            i: api::TeamKnowledgeTurnOffInput,
        ) -> Result<Done, ServiceError> {
            self.team()?
                .turn_off(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }

        pub(crate) async fn tk_check_now(
            &self,
            i: api::TeamKnowledgeCheckNowInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            self.own_folder_only(i.folder_id, i.ws)?;
            let s = self
                .team()?
                .check_now(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_get(
            &self,
            i: api::TeamKnowledgeGetInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            self.own_folder_only(i.folder_id, i.ws)?;
            let s = self
                .team()?
                .get(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_share(
            &self,
            i: api::TeamKnowledgeShareInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let s = self
                .team()?
                .share(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_workspace_share_preview(
            &self,
            i: api::TeamKnowledgeWorkspaceSharePreviewInput,
        ) -> Result<api::TeamWorkspaceSharePreview, ServiceError> {
            let choice = self.restate(i.choice)?;
            let p = self
                .team()?
                .workspace_share_preview(i.ws, choice)
                .await
                .map_err(|e| self.module_error(e))?;
            self.restate(p)
        }

        pub(crate) async fn tk_share_workspace(
            &self,
            i: api::TeamKnowledgeShareWorkspaceInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let choice = self.restate(i.choice)?;
            let results = self.restate(i.results)?;
            let s = self
                .team()?
                .share_workspace(i.ws, choice, results)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_workspace_open_preview(
            &self,
            i: api::TeamKnowledgeWorkspaceOpenPreviewInput,
        ) -> Result<api::TeamWorkspaceOpenPreview, ServiceError> {
            let p = self
                .team()?
                .workspace_open_preview(i.url, i.branch, i.path)
                .await
                .map_err(|e| self.module_error(e))?;
            self.restate(p)
        }

        pub(crate) async fn tk_open_workspace(
            &self,
            i: api::TeamKnowledgeOpenWorkspaceInput,
        ) -> Result<api::TeamWorkspaceOpened, ServiceError> {
            let opened = self
                .team()?
                .open_workspace(i.url, i.branch, i.path, i.name)
                .await
                .map_err(|e| self.module_error(e))?;
            let status = self.team_status(opened.status)?;
            Ok(api::TeamWorkspaceOpened {
                workspace_id: opened.workspace_id,
                status,
            })
        }

        pub(crate) async fn tk_pending_grants(
            &self,
            i: api::TeamKnowledgePendingGrantsInput,
        ) -> Result<Vec<api::TeamPendingGrantView>, ServiceError> {
            let g = self
                .team()?
                .pending_grants(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.restate(g)
        }

        pub(crate) async fn tk_allow_grant(
            &self,
            i: api::TeamKnowledgeAllowGrantInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let s = self
                .team()?
                .allow_grant(i.ws, i.grant_id)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_decline_grant(
            &self,
            i: api::TeamKnowledgeDeclineGrantInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let s = self
                .team()?
                .decline_grant(i.ws, i.grant_id)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_activity(
            &self,
            i: api::TeamKnowledgeActivityInput,
        ) -> Result<ordin_types::Page<api::TeamActivityView>, ServiceError> {
            let page = self
                .team()?
                .activity(i.ws, i.cursor)
                .await
                .map_err(|e| self.module_error(e))?;
            self.restate(page)
        }

        /// **Change…** after sharing (TeamKnowledgeService.md TW-K, amended January 2026 by the
        /// user's decision).
        // spec: ordin-desktop/components/OrdinDesktop#DSK-TW-3
        pub(crate) async fn tk_results_choices(
            &self,
            i: api::TeamKnowledgeResultsChoicesInput,
        ) -> Result<api::TeamResultsChoices, ServiceError> {
            let choices = self
                .team()?
                .results_choices(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            self.restate(choices)
        }

        pub(crate) async fn tk_set_results_place(
            &self,
            i: api::TeamKnowledgeSetResultsPlaceInput,
        ) -> Result<api::TeamKnowledgeStatus, ServiceError> {
            let results = self.restate(i.results)?;
            let s = self
                .team()?
                .set_results_place(i.ws, results)
                .await
                .map_err(|e| self.module_error(e))?;
            self.team_status(s)
        }

        pub(crate) async fn tk_leave_workspace(
            &self,
            i: api::TeamKnowledgeLeaveWorkspaceInput,
        ) -> Result<Done, ServiceError> {
            self.team()?
                .leave_workspace(i.ws)
                .await
                .map_err(|e| self.module_error(e))?;
            Ok(Done {})
        }
    }
}

/// A build without `team-knowledge` answers no `team_knowledge.*` command (DSK-TW-1).
#[cfg(not(feature = "team-knowledge"))]
mod absent {
    use super::*;

    macro_rules! absent {
        ($($name:ident: $input:ty => $result:ty;)*) => {
            impl DesktopServiceApi {
                $(
                    pub(crate) async fn $name(&self, _input: $input) -> Result<$result, ServiceError> {
                        Err(self.not_answered())
                    }
                )*
            }
        };
    }

    absent! {
        tk_status: api::TeamKnowledgeStatusInput => api::TeamKnowledgeStatus;
        tk_candidate_links: api::TeamKnowledgeCandidateLinksInput => Vec<api::SourceLink>;
        tk_turn_on: api::TeamKnowledgeTurnOnInput => api::TeamKnowledgeStatus;
        tk_turn_off: api::TeamKnowledgeTurnOffInput => Done;
        tk_check_now: api::TeamKnowledgeCheckNowInput => api::TeamKnowledgeStatus;
        tk_get: api::TeamKnowledgeGetInput => api::TeamKnowledgeStatus;
        tk_share: api::TeamKnowledgeShareInput => api::TeamKnowledgeStatus;
        tk_workspace_share_preview: api::TeamKnowledgeWorkspaceSharePreviewInput => api::TeamWorkspaceSharePreview;
        tk_share_workspace: api::TeamKnowledgeShareWorkspaceInput => api::TeamKnowledgeStatus;
        tk_workspace_open_preview: api::TeamKnowledgeWorkspaceOpenPreviewInput => api::TeamWorkspaceOpenPreview;
        tk_open_workspace: api::TeamKnowledgeOpenWorkspaceInput => api::TeamWorkspaceOpened;
        tk_pending_grants: api::TeamKnowledgePendingGrantsInput => Vec<api::TeamPendingGrantView>;
        tk_allow_grant: api::TeamKnowledgeAllowGrantInput => api::TeamKnowledgeStatus;
        tk_decline_grant: api::TeamKnowledgeDeclineGrantInput => api::TeamKnowledgeStatus;
        tk_activity: api::TeamKnowledgeActivityInput => ordin_types::Page<api::TeamActivityView>;
        tk_results_choices: api::TeamKnowledgeResultsChoicesInput => api::TeamResultsChoices;
        tk_set_results_place: api::TeamKnowledgeSetResultsPlaceInput => api::TeamKnowledgeStatus;
        tk_leave_workspace: api::TeamKnowledgeLeaveWorkspaceInput => Done;
    }
}
