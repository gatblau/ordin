//! Removing, readying, verifying and changing instances (PackageCatalogue.md §B, §E, §F, §I) and
//! the trust changes the program applies (§L).

use std::path::Path;

use ordin_agent_folders::AfmError;
use ordin_checker::{InstallCheck, InstallVerdict, TrustChanges};
use ordin_installers::{InstallError, InstallRequest, VerifyOutcome};
use ordin_resolver::model::{RepositoryRef, SourceScope};
use ordin_resolver::ResolveError;
use ordin_types::domain::agents::{AgentInstanceStatus, StoppedBy};
use ordin_types::AgentInstanceId;
use serde_json::json;

use crate::add::apply_inspected;
use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::request::{resolve_request, selected_by, PlanInputs};
use crate::store::{self, ChoiceSource, InstanceRow, StoredChoice};
use crate::types::{
    AgentFolderFailure, AgentFolderReport, AgentNotice, ChangeChoiceInput, InstalledAgent,
    RemoveAgentInput,
};
use ordin_resolver::model::ChosenBy;

/// A provider failure while removing or readying: retryable ones ask the person to try again, the
/// rest send the problem to IT.
fn provider_failure(retryable: bool, detail: String) -> PackageError {
    if retryable {
        PackageError::install_failed(detail)
    } else {
        PackageError::folder_unusable(detail)
    }
}

/// The scope a trial of an own draft is repaired in: it has no merged repository (SA-CLOSE-03), so
/// only the bundled catalogue is named.
///
/// A trial has no `source_scope` (`PKG_TRIAL_NO_SCOPE`); the bundled-only scope lets a trial that
/// needs no repair start a session (§F).
fn trial_scope() -> SourceScope {
    SourceScope {
        home: RepositoryRef::Bundled,
        profile: Vec::new(),
        include_bundled: true,
    }
}

impl PackageCatalogue {
    async fn repair_scope(&self, row: &InstanceRow) -> Result<SourceScope, PackageError> {
        if row.is_trial_of_own_draft {
            return Ok(trial_scope());
        }
        self.scope_of(row).await
    }

    /// §E: remove an agent after the person confirmed (the confirmation is the screen's).
    // spec: ordin-desktop/components/PackageCatalogue#PKG-REC-3
    pub async fn remove(&self, input: RemoveAgentInput) -> Result<(), PackageError> {
        if crate::types::is_bundled(input.agent_instance_id) {
            return Err(PackageError::bundled_agent_fixed());
        }
        let lock = self.lock_for(input.agent_instance_id);
        let _held = lock.lock().await;
        let id = input.agent_instance_id;
        self.live_row(id).await?;
        if let Some(workspace) = input.workspace_id {
            let current = {
                let mut conn = self.conn().await?;
                store::workspaces_of(&mut conn, id).await?
            };
            if !current.contains(&workspace) {
                return Err(PackageError::instance_unknown());
            }
            let remaining: Vec<_> = current.into_iter().filter(|w| *w != workspace).collect();
            if !remaining.is_empty() {
                self.inner
                    .ports
                    .inspector
                    .set_workspaces(id, remaining)
                    .await
                    .map_err(|e| PackageError::install_failed(e.to_string()))?;
                let mut conn = self.conn().await?;
                store::remove_membership(&mut conn, id, workspace).await?;
                return Ok(());
            }
        }
        self.remove_instance_locked(id).await
    }

    /// The full removal of §E; the caller holds the instance lock or the add lock.
    pub(crate) async fn remove_instance(&self, id: AgentInstanceId) -> Result<(), PackageError> {
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        self.remove_instance_locked(id).await
    }

    async fn remove_instance_locked(&self, id: AgentInstanceId) -> Result<(), PackageError> {
        let mut row = self.live_row(id).await?;
        let ports = &self.inner.ports;
        match ports.installers.uninstall(id).await {
            Ok(()) | Err(InstallError::NotInstalled) => {}
            Err(error) => return Err(provider_failure(error.retryable(), error.to_string())),
        }
        if let Err(error) = ports.inspector.forget(id).await {
            tracing::warn!(
                component = "package_catalogue",
                agent_instance_id = id.to_string().as_str(),
                code = error.code(),
                "agent.forget_failed"
            );
        }
        if let Err(error) = ports.checker.forget_instance(id).await {
            tracing::warn!(
                component = "package_catalogue",
                agent_instance_id = id.to_string().as_str(),
                code = error.code(),
                "agent.forget_failed"
            );
        }
        match ports.folders.remove(id).await {
            Ok(()) | Err(AfmError::FolderNotFound { .. }) => {}
            Err(error) => return Err(provider_failure(error.retryable(), error.to_string())),
        }
        let workspace = self.first_workspace(id).await;
        row.status = AgentInstanceStatus::Removed;
        {
            let mut tx = self.pool()?.begin().await?;
            self.save(&mut tx, &row).await?;
            store::delete_pending(&mut tx, id).await?;
            tx.commit().await?;
        }
        self.prune().await;
        self.journal(
            "agent_removed",
            workspace,
            "An agent was removed.",
            json!({ "agent_instance_id": id.to_string() }),
        )
        .await;
        self.notify(AgentNotice::Removed {
            agent_instance_id: id,
        });
        tracing::info!(
            component = "package_catalogue",
            agent_instance_id = id.to_string().as_str(),
            "agent.removed"
        );
        Ok(())
    }

    /// §F: before a session starts on an instance.
    pub async fn ensure_ready(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError> {
        let row = {
            let mut conn = self.conn().await?;
            store::get_instance(&mut conn, id)
                .await?
                .ok_or_else(PackageError::instance_unknown)?
        };
        if !matches!(
            row.status,
            AgentInstanceStatus::Ready | AgentInstanceStatus::Updating
        ) {
            return Err(PackageError::instance_not_ready(
                "the instance is not ready",
            ));
        }
        let scope = self.repair_scope(&row).await?;
        match self
            .inner
            .ports
            .installers
            .verify_and_repair(id, &scope)
            .await
        {
            Ok(outcome) => {
                if outcome.repaired {
                    self.repaired(&row).await;
                }
                self.get(id).await
            }
            Err(error) if error.retryable() => Err(PackageError::install_failed(error.to_string())),
            Err(error) => Err(PackageError::folder_unusable(error.to_string())),
        }
    }

    async fn repaired(&self, row: &InstanceRow) {
        let workspace = self.first_workspace(row.id).await;
        self.journal(
            "agent_repaired",
            workspace,
            "An agent's folder was rebuilt from its lock file.",
            json!({ "agent_instance_id": row.id.to_string() }),
        )
        .await;
        tracing::info!(
            component = "package_catalogue",
            agent_instance_id = row.id.to_string().as_str(),
            "agent.repaired"
        );
    }

    /// §I: verify every `ready` or `updating` instance's folder against its lock file.
    pub async fn verify_agent_folders(
        &self,
        repair: bool,
    ) -> Result<AgentFolderReport, PackageError> {
        let rows = {
            let mut conn = self.conn().await?;
            store::verifiable_instances(&mut conn).await?
        };
        let mut report = AgentFolderReport {
            checked: u32::try_from(rows.len()).unwrap_or(u32::MAX),
            repaired: 0,
            failed: Vec::new(),
        };
        for row in rows {
            let failure = |code: &str| AgentFolderFailure {
                agent_name: row.display_name.clone(),
                code: code.to_owned(),
            };
            if !repair {
                match self.inner.ports.installers.verify(row.id).await {
                    Ok(VerifyOutcome::Verified) => {}
                    Ok(VerifyOutcome::Mismatch { .. }) => {
                        report.failed.push(failure("PKG_FOLDER_MISMATCH"));
                    }
                    Err(_) => report.failed.push(failure("PKG_FOLDER_UNUSABLE")),
                }
                continue;
            }
            let outcome = match self.repair_scope(&row).await {
                Ok(scope) => self
                    .inner
                    .ports
                    .installers
                    .verify_and_repair(row.id, &scope)
                    .await
                    .ok(),
                Err(_) => None,
            };
            match outcome {
                Some(outcome) => {
                    if outcome.repaired {
                        report.repaired += 1;
                        self.repaired(&row).await;
                    }
                }
                None => report.failed.push(failure("PKG_FOLDER_UNUSABLE")),
            }
        }
        Ok(report)
    }

    /// §B: the person changes a pack choice (**Change**).
    pub async fn change_choice(
        &self,
        input: ChangeChoiceInput,
    ) -> Result<InstalledAgent, PackageError> {
        // spec: ordin-desktop/components/PackageCatalogue#PKG-REC-3
        if crate::types::is_bundled(input.agent_instance_id) {
            return Err(PackageError::bundled_agent_fixed());
        }
        self.apply_choice(input, ChoiceSource::PersonChange).await
    }

    /// §B: the person answers a pack question the agent asked (decision 64).
    pub async fn record_answer(
        &self,
        input: ChangeChoiceInput,
    ) -> Result<InstalledAgent, PackageError> {
        self.apply_choice(input, ChoiceSource::AgentAsked).await
    }

    async fn apply_choice(
        &self,
        input: ChangeChoiceInput,
        source: ChoiceSource,
    ) -> Result<InstalledAgent, PackageError> {
        let id = input.agent_instance_id;
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        let mut row = self.live_row(id).await?;
        if row.status != AgentInstanceStatus::Ready {
            return Err(PackageError::instance_not_ready(
                "the instance is not ready",
            ));
        }
        if row.is_trial_of_own_draft {
            return Err(PackageError::trial_no_scope());
        }
        if input.choice.packs.is_empty() {
            return Err(PackageError::choice_not_allowed("a choice names no pack"));
        }
        let scope = self.scope_of(&row).await?;
        let previous = {
            let mut conn = self.conn().await?;
            store::choices_of(&mut conn, id).await?
        };
        let chosen = StoredChoice {
            rule_index: input.choice.rule_index,
            packs: input.choice.packs.clone(),
            source,
        };
        let mut choices: Vec<StoredChoice> = previous
            .iter()
            .filter(|c| c.rule_index != chosen.rule_index)
            .cloned()
            .collect();
        choices.push(chosen.clone());
        let inputs = PlanInputs {
            languages: row.workspace_languages.clone(),
            locale: row.interface_locale.clone(),
            harness: row.harness,
            choices,
        };
        let request = resolve_request(
            scope.clone(),
            &row.agent_identifier,
            Some(row.package_version.clone()),
            &inputs,
        );
        let plan =
            self.inner
                .ports
                .resolver
                .resolve(&request)
                .await
                .map_err(|error| match error {
                    ResolveError::PackNotAllowed { .. } => {
                        PackageError::choice_not_allowed("a pack is not one the rule allows")
                    }
                    other if other.retryable() => {
                        PackageError::check_unavailable(other.to_string())
                    }
                    other => PackageError::choice_not_allowed(other.to_string()),
                })?;
        // The Resolver ignores an answer for a rule the agent does not have, so the choice is
        // accepted only when every pack it names appears in the plan as chosen by this person.
        let expected = match source {
            ChoiceSource::PersonChange => ChosenBy::PersonChange,
            ChoiceSource::AgentAsked => ChosenBy::PersonAnswer,
        };
        if !chosen
            .packs
            .iter()
            .all(|p| selected_by(&plan, p) == Some(expected))
        {
            return Err(PackageError::choice_not_allowed(
                "the plan does not use the chosen packs",
            ));
        }
        let report = self
            .inner
            .ports
            .checker
            .check_for_install(&InstallCheck {
                plan: plan.clone(),
                scope,
                agent_instance_id: Some(id),
                source: None,
            })
            .await
            .map_err(|e| PackageError::check_unavailable(e.to_string()))?;
        if !matches!(report.verdict, InstallVerdict::Clean) {
            return Err(PackageError::choice_not_allowed(
                "the changed agent does not pass its checks",
            ));
        }
        {
            let mut conn = self.conn().await?;
            store::put_choice(&mut conn, id, &chosen).await?;
        }
        match self.reinstall(&mut row, &request, source).await {
            Ok(()) => self.get(id).await,
            Err(error) => {
                let mut conn = self.conn().await?;
                match previous.iter().find(|c| c.rule_index == chosen.rule_index) {
                    Some(old) => store::put_choice(&mut conn, id, old).await?,
                    None => store::delete_choice(&mut conn, id, chosen.rule_index).await?,
                }
                Err(error)
            }
        }
    }

    /// §B: rebuild, install between turns when a session runs, re-inspect, `ready`, prune.
    async fn reinstall(
        &self,
        row: &mut InstanceRow,
        request: &ordin_resolver::ResolveRequest,
        source: ChoiceSource,
    ) -> Result<(), PackageError> {
        let ports = &self.inner.ports;
        let id = row.id;
        let bundle = ports
            .resolver
            .build(request)
            .await
            .map_err(|e| PackageError::install_failed(e.to_string()))?;
        let _hold = self.hold_hash(&bundle.plan.plan_hash);
        let install = InstallRequest {
            agent_instance_id: id,
            harness: row.harness,
            plan_hash: bundle.plan.plan_hash.clone(),
        };
        // C-64: the change takes effect at the next turn when the agent asked or a session runs.
        let outcome = if source == ChoiceSource::AgentAsked || self.inner.activity.running(id) {
            ports.installers.update_in_session(&install).await
        } else {
            ports.installers.update(&install).await
        }
        .map_err(|e| PackageError::install_failed(e.to_string()))?;
        let inspected = self
            .inspect_and_decide(id, Path::new(&row.agent_folder_path), &bundle, None)
            .await?;
        apply_inspected(row, &bundle, &inspected);
        row.status = AgentInstanceStatus::Ready;
        {
            let mut conn = self.conn().await?;
            self.save(&mut conn, row).await?;
        }
        tracing::info!(
            component = "package_catalogue",
            agent_instance_id = id.to_string().as_str(),
            restart_required = outcome.restart_required,
            "agent.choice_applied"
        );
        self.prune().await;
        Ok(())
    }

    // spec: ordin-desktop/components/PackageCatalogue#SA-CLOSE-02
    /// §L: the program, after the Checker's `reevaluate_trust`.
    pub async fn apply_trust_changes(&self, changes: &TrustChanges) -> Result<(), PackageError> {
        for id in &changes.lost {
            self.trust_lost(*id).await?;
        }
        for id in &changes.gained {
            self.trust_gained(*id).await?;
        }
        Ok(())
    }

    async fn trust_lost(&self, id: AgentInstanceId) -> Result<(), PackageError> {
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        let row = {
            let mut conn = self.conn().await?;
            store::get_instance(&mut conn, id).await?
        };
        let Some(mut row) = row.filter(|r| {
            matches!(
                r.status,
                AgentInstanceStatus::Ready
                    | AgentInstanceStatus::Updating
                    | AgentInstanceStatus::UpdateAwaitingChoice
            )
        }) else {
            tracing::info!(
                component = "package_catalogue",
                agent_instance_id = id.to_string().as_str(),
                "agent.trust_change_skipped"
            );
            return Ok(());
        };
        let reason = format!(
            "{} stopped: your company's rules no longer allow one of the programs it uses. Your other agents still work.",
            row.display_name
        );
        row.status = AgentInstanceStatus::Refused;
        row.stopped_by = Some(StoppedBy::Trust);
        row.stopped_reason = Some(reason.clone());
        {
            let mut tx = self.pool()?.begin().await?;
            self.save(&mut tx, &row).await?;
            store::delete_pending(&mut tx, id).await?;
            tx.commit().await?;
        }
        self.notify(AgentNotice::Stopped {
            agent_instance_id: id,
            reason,
        });
        let workspace = self.first_workspace(id).await;
        self.journal(
            "agent_stopped",
            workspace,
            "An agent stopped because its programs are no longer trusted.",
            json!({ "agent_instance_id": id.to_string(), "stopped_by": "trust" }),
        )
        .await;
        tracing::info!(
            component = "package_catalogue",
            agent_instance_id = id.to_string().as_str(),
            "agent.stopped"
        );
        Ok(())
    }

    async fn trust_gained(&self, id: AgentInstanceId) -> Result<(), PackageError> {
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        let row = {
            let mut conn = self.conn().await?;
            store::get_instance(&mut conn, id).await?
        };
        let Some(mut row) = row.filter(|r| {
            r.status == AgentInstanceStatus::Refused && r.stopped_by == Some(StoppedBy::Trust)
        }) else {
            tracing::info!(
                component = "package_catalogue",
                agent_instance_id = id.to_string().as_str(),
                resumed = false,
                "agent.trust_gained"
            );
            return Ok(());
        };
        match self.inner.ports.installers.verify(id).await {
            Ok(VerifyOutcome::Verified) => {}
            Ok(VerifyOutcome::Mismatch { .. }) | Err(_) => {
                tracing::warn!(
                    component = "package_catalogue",
                    agent_instance_id = id.to_string().as_str(),
                    "agent.trust_gained_folder_unusable"
                );
                return Ok(());
            }
        }
        row.status = AgentInstanceStatus::Ready;
        row.stopped_by = None;
        row.stopped_reason = None;
        {
            let mut conn = self.conn().await?;
            self.save(&mut conn, &row).await?;
        }
        self.notify(AgentNotice::Resumed {
            agent_instance_id: id,
        });
        let workspace = self.first_workspace(id).await;
        self.journal(
            "agent_resumed",
            workspace,
            "An agent can run again because its programs are trusted again.",
            json!({ "agent_instance_id": id.to_string() }),
        )
        .await;
        tracing::info!(
            component = "package_catalogue",
            agent_instance_id = id.to_string().as_str(),
            resumed = true,
            "agent.trust_gained"
        );
        Ok(())
    }
}
