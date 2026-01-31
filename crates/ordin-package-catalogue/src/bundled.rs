//! The agents bundled with the application: Ordin's own agent and the agent maker, owned at fixed
//! instance ids and added from the bundled catalogue on first use (PackageCatalogue.md, amendment
//! January 2026, PKG-REC-1 to PKG-REC-4).

use ordin_catalogue::PartStateLabel;
use ordin_checker::{InstallCheck, InstallVerdict};
use ordin_resolver::model::{slug_from_id, PartKind, RepositoryRef, BUNDLED_REPOSITORY_ID};
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::AgentInstanceId;

use crate::add::{part_names, NewInstance};
use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::request::{map_resolve_error, resolve_request, Failure, PlanInputs};
use crate::store;
use crate::types::{BundledAgent, InstalledAgent};

impl PackageCatalogue {
    /// The installed instance of a bundled agent, added from the bundled catalogue on the first call.
    /// Called by the program at start (OrdinDesktop.md DSK-REC-24).
    // spec: ordin-desktop/components/PackageCatalogue#PKG-REC-2
    pub async fn bundled_instance(
        &self,
        agent: BundledAgent,
        interface_locale: &str,
    ) -> Result<InstalledAgent, PackageError> {
        let _serial = self.inner.bundled_lock.lock().await;
        let id = agent.instance_id();
        let existing = {
            let mut conn = self.conn().await?;
            store::get_instance(&mut conn, id).await?
        };
        if let Some(row) = existing {
            match row.status {
                AgentInstanceStatus::Ready
                | AgentInstanceStatus::Updating
                | AgentInstanceStatus::UpdateAwaitingChoice => {
                    return self.installed_agent(row, None).await;
                }
                AgentInstanceStatus::Refused if row.stopped_by.is_some() => {
                    return Err(PackageError::bundled_agent_refused(
                        row.stopped_reason.unwrap_or_default(),
                    ));
                }
                AgentInstanceStatus::Refused
                | AgentInstanceStatus::Installing
                | AgentInstanceStatus::Removed => self.clear_bundled(id).await?,
            }
        }
        let identifier = agent.identifier().ok_or_else(|| {
            PackageError::bundled_agent_missing("the bundled agent's identifier is not supplied")
        })?;
        let ports = &self.inner.ports;
        let missing = || {
            tracing::error!(
                component = "package_catalogue",
                agent = agent.log_name(),
                identifier,
                "agent.bundled_missing"
            );
            PackageError::bundled_agent_missing(
                "the bundled catalogue holds no current, offered WorkspaceProfile with the identifier",
            )
        };
        let view = match ports
            .catalogue
            .find_part(
                BUNDLED_REPOSITORY_ID,
                PartKind::WorkspaceProfile,
                identifier,
                None,
            )
            .await
        {
            Ok(view) => view,
            Err(error) if matches!(error.code, "CAT_PART_UNKNOWN" | "CAT_REPOSITORY_UNKNOWN") => {
                return Err(missing());
            }
            Err(error) => {
                return Err(PackageError::library_unavailable(
                    error.retryable,
                    error.code,
                ))
            }
        };
        if view.state_label != PartStateLabel::Current || !view.offered {
            return Err(missing());
        }
        let scope = ports
            .catalogue
            .source_scope(RepositoryRef::Bundled)
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
        let inputs = PlanInputs {
            languages: Vec::new(),
            locale: interface_locale.to_owned(),
            harness: self.inner.harness.default_harness(),
            choices: Vec::new(),
        };
        let request = resolve_request(
            scope.clone(),
            identifier,
            Some(view.part.version.clone()),
            &inputs,
        );
        let plan = match ports.resolver.resolve(&request).await {
            Ok(plan) => plan,
            Err(error) => {
                let detail = match map_resolve_error(&error) {
                    Failure::PackMissing(label) => format!("a required pack is missing: {label}"),
                    Failure::Unsafe(code) => format!("the agent cannot be added safely: {code}"),
                    Failure::Err(error) => return Err(error),
                };
                self.refused(None, identifier, "bundled_agent_refused", None)
                    .await;
                return Err(PackageError::bundled_agent_refused(detail));
            }
        };
        let report = ports
            .checker
            .check_for_install(&InstallCheck {
                plan: plan.clone(),
                scope,
                agent_instance_id: Some(id),
                source: None,
            })
            .await
            .map_err(|e| PackageError::check_unavailable(e.to_string()))?;
        let check_run_id = report.report.check_run_id.to_string();
        let refusal = match report.verdict {
            InstallVerdict::Clean => None,
            InstallVerdict::Failed => Some("the agent fails its checks".to_owned()),
            InstallVerdict::NeedsItApproval { parts } => Some(format!(
                "the agent needs IT approval for: {}",
                part_names(&parts).join(", ")
            )),
        };
        if let Some(detail) = refusal {
            self.refused(None, identifier, "check_failed", Some(&check_run_id))
                .await;
            return Err(PackageError::bundled_agent_refused(detail));
        }
        let spec = NewInstance {
            id,
            workspace: None,
            display_name: view.part.name.clone(),
            package_name: slug_from_id(identifier),
            package_version: view.part.version.clone(),
            repository: Some((BUNDLED_REPOSITORY_ID, view.part.commit.clone())),
            identifier: identifier.to_owned(),
            draft: None,
            inputs,
            request,
            plan,
            check_run_id,
        };
        let installed = self.install_new(spec).await?;
        tracing::info!(
            component = "package_catalogue",
            agent = agent.log_name(),
            agent_instance_id = %id,
            "agent.bundled_added"
        );
        Ok(installed)
    }

    /// PKG-REC-2 step 2: a failed, interrupted or removed bundled install is cleaned and its row
    /// deleted, so it is installed again under the same id. A cleaning failure is logged only.
    async fn clear_bundled(&self, id: AgentInstanceId) -> Result<(), PackageError> {
        let ports = &self.inner.ports;
        if let Err(error) = ports.installers.uninstall(id).await {
            tracing::warn!(component = "package_catalogue", agent_instance_id = %id, code = error.code(), "agent.bundled_cleanup_failed");
        }
        if let Err(error) = ports.folders.remove(id).await {
            tracing::warn!(component = "package_catalogue", agent_instance_id = %id, code = %error, "agent.bundled_cleanup_failed");
        }
        let mut conn = self.conn().await?;
        store::delete_instance(&mut conn, id).await
    }
}
