//! Preview and add (PackageCatalogue.md §A), the install pipeline both `add` and `add_trial` run
//! (§A6–A10) and the trial of an own draft (§G).

use std::path::PathBuf;
use std::time::Instant;

use ordin_catalogue::{CatalogueError, PartStateLabel, PartView};
use ordin_checker::{ExecutablePart, InstallCheck, InstallVerdict};
use ordin_installers::InstallRequest;
use ordin_package_inspector::InspectOutcome;
use ordin_resolver::model::{
    slug_from_id, PartKind, RepositoryRef, Resolution, ResolutionPlan, SourceOverride, SourceScope,
};
use ordin_resolver::{Bundle, ResolveRequest};
use ordin_types::domain::agents::{
    AgentInstanceStatus, AuditResult, Authority, ExecutablePartDecision,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentDraftId, AgentInstanceId, AgentRepositoryId, WorkspaceId};
use serde_json::json;

use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::request::{home_of, map_resolve_error, resolve_request, Failure, PlanInputs};
use crate::store::{self, ChoiceSource, InstanceRow, StoredChoice};
use crate::types::{
    AddAgentInput, AddOutcome, AddPreview, Availability, InstalledAgent, TrialInput,
};

/// Everything an install of a new instance needs.
pub(crate) struct NewInstance {
    pub id: AgentInstanceId,
    /// `None` for a bundled instance, which serves no workspace (PKG-REC-2 step 6).
    pub workspace: Option<WorkspaceId>,
    pub display_name: String,
    pub package_name: String,
    pub package_version: String,
    pub repository: Option<(AgentRepositoryId, String)>,
    pub identifier: String,
    pub draft: Option<AgentDraftId>,
    pub inputs: PlanInputs,
    pub request: ResolveRequest,
    pub plan: ResolutionPlan,
    pub check_run_id: String,
}

/// What a successful install and inspection produced.
pub(crate) struct Installed {
    pub folder_path: PathBuf,
    pub bundle: Bundle,
    pub inspected: Inspected,
}

pub(crate) struct Inspected {
    pub outcome: InspectOutcome,
    pub decisions: Vec<ExecutablePartDecision>,
}

/// A failed attempt of §A7–A8 and whether it reached the installer (§A10).
struct StepError {
    error: PackageError,
    reached_installer: bool,
}

/// The pieces §A1–A3 produce for a library agent.
struct Prepared {
    view: PartView,
    scope: SourceScope,
    request: ResolveRequest,
    inputs: PlanInputs,
}

fn catalogue_failure(error: &CatalogueError) -> PackageError {
    PackageError::library_unavailable(error.retryable, error.code)
}

impl PackageCatalogue {
    /// §A1–A3 and §A0.1–A0.2.
    async fn prepare_add(&self, input: &AddAgentInput) -> Result<Prepared, PackageError> {
        let view = self
            .inner
            .ports
            .catalogue
            .part(input.agent)
            .await
            .map_err(|e| match e.code {
                "CAT_PART_UNKNOWN" => PackageError::agent_unknown(),
                _ => catalogue_failure(&e),
            })?;
        if view.part.kind != PartKind::WorkspaceProfile {
            return Err(PackageError::not_an_agent());
        }
        if !view.offered || view.state_label == PartStateLabel::Withdrawn {
            return Err(PackageError::agent_not_offered("the agent is not offered"));
        }
        // PKG-OWN-1: Ordin and the agent maker are ordin's own; neither is added to a workspace.
        if crate::types::is_own_agent(view.part.agent_repository_id, &view.part.identifier) {
            return Err(PackageError::agent_not_offered("ordin's own agent"));
        }
        if view.state_label == PartStateLabel::NotForNewAgents {
            return Err(PackageError::deprecated_as_agent());
        }
        {
            let mut conn = self.conn().await?;
            if store::same_agent_in_workspace(&mut conn, input.workspace_id, &view.part.identifier)
                .await?
                .is_some()
            {
                return Err(PackageError::already_added());
            }
        }
        let choices = stored_choices(&input.choices)?;
        let scope = self
            .inner
            .ports
            .catalogue
            .source_scope(home_of(Some(view.part.agent_repository_id)))
            .await
            .map_err(|e| catalogue_failure(&e))?;
        let inputs = PlanInputs {
            languages: input.workspace_languages.clone(),
            locale: input.interface_locale.clone(),
            harness: self.inner.harness.default_harness(),
            choices,
        };
        let request = resolve_request(
            scope.clone(),
            &view.part.identifier,
            Some(view.part.version.clone()),
            &inputs,
        );
        Ok(Prepared {
            view,
            scope,
            request,
            inputs,
        })
    }

    /// §A1–A5: what adding the agent would do, creating nothing.
    pub async fn preview_add(&self, input: AddAgentInput) -> Result<AddPreview, PackageError> {
        let prepared = self.prepare_add(&input).await?;
        let preview = |available: Availability| AddPreview {
            agent_name: prepared.view.part.name.clone(),
            from: prepared.view.from.clone(),
            uses: Vec::new(),
            asks: Vec::new(),
            available,
        };
        let plan = match self.inner.ports.resolver.resolve(&prepared.request).await {
            Ok(plan) => plan,
            Err(error) => {
                return match map_resolve_error(&error) {
                    Failure::PackMissing(pack_class_label) => {
                        Ok(preview(Availability::PackMissing { pack_class_label }))
                    }
                    Failure::Unsafe(_) => Ok(preview(Availability::Unsafe)),
                    Failure::Err(error) => Err(error),
                };
            }
        };
        let report = self
            .inner
            .ports
            .checker
            .check_for_install(&InstallCheck {
                plan: plan.clone(),
                scope: prepared.scope.clone(),
                agent_instance_id: None,
                source: None,
            })
            .await
            .map_err(|e| PackageError::check_unavailable(e.to_string()))?;
        match report.verdict {
            InstallVerdict::Failed => Ok(preview(Availability::Unsafe)),
            InstallVerdict::NeedsItApproval { parts } => {
                Ok(preview(Availability::NeedsItApproval {
                    parts: part_names(&parts),
                }))
            }
            InstallVerdict::Clean => {
                let repos = self.repo_index().await.unwrap_or_default();
                let mut out = preview(Availability::Available);
                out.uses = self.uses_of(&plan, &repos).await;
                out.asks = plan.selections.pending_choices.clone();
                Ok(out)
            }
        }
    }

    // spec: ordin-desktop/components/PackageCatalogue#SA-CLOSE-01
    /// §A: **Add it**.
    pub async fn add(&self, input: AddAgentInput) -> Result<AddOutcome, PackageError> {
        let started = Instant::now();
        let _serial = self.inner.add_lock.lock().await;
        let prepared = self.prepare_add(&input).await?;
        let identifier = prepared.view.part.identifier.clone();
        let id = AgentInstanceId::new();
        let plan = match self.inner.ports.resolver.resolve(&prepared.request).await {
            Ok(plan) => plan,
            Err(error) => {
                return match map_resolve_error(&error) {
                    Failure::PackMissing(pack_class_label) => {
                        Ok(AddOutcome::PackMissing { pack_class_label })
                    }
                    Failure::Unsafe(code) => {
                        self.refused(Some(input.workspace_id), &identifier, code, None)
                            .await;
                        Ok(AddOutcome::Unsafe)
                    }
                    Failure::Err(error) => Err(error),
                };
            }
        };
        // SA-CLOSE-01: the id is minted before the check so the Checker stores its executable parts.
        let report = self
            .inner
            .ports
            .checker
            .check_for_install(&InstallCheck {
                plan: plan.clone(),
                scope: prepared.scope.clone(),
                agent_instance_id: Some(id),
                source: None,
            })
            .await
            .map_err(|e| PackageError::check_unavailable(e.to_string()))?;
        let check_run_id = report.report.check_run_id.to_string();
        match report.verdict {
            InstallVerdict::Failed => {
                self.refused(
                    Some(input.workspace_id),
                    &identifier,
                    "check_failed",
                    Some(&check_run_id),
                )
                .await;
                tracing::info!(component = "package_catalogue", agent_identifier = %identifier, verdict = "failed", "agent.add_refused");
                return Ok(AddOutcome::Unsafe);
            }
            InstallVerdict::NeedsItApproval { parts } => {
                tracing::info!(component = "package_catalogue", agent_identifier = %identifier, verdict = "needs_it_approval", "agent.add_refused");
                return Ok(AddOutcome::NeedsItApproval {
                    parts: part_names(&parts),
                });
            }
            InstallVerdict::Clean => {}
        }
        let part = &prepared.view.part;
        let spec = NewInstance {
            id,
            workspace: Some(input.workspace_id),
            display_name: part.name.clone(),
            package_name: slug_from_id(&part.identifier),
            package_version: part.version.clone(),
            repository: Some((part.agent_repository_id, part.commit.clone())),
            identifier: identifier.clone(),
            draft: None,
            inputs: prepared.inputs,
            request: prepared.request,
            plan,
            check_run_id,
        };
        let agent = self.install_new(spec).await?;
        tracing::info!(
            component = "package_catalogue",
            agent_instance_id = %id,
            agent_identifier = %identifier,
            outcome = "ready",
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "agent.add"
        );
        Ok(AddOutcome::Ready { agent })
    }

    /// Journal entry `agent_add_refused`.
    pub(crate) async fn refused(
        &self,
        workspace: Option<WorkspaceId>,
        identifier: &str,
        code: &str,
        check_run_id: Option<&str>,
    ) {
        self.journal(
            "agent_add_refused",
            workspace,
            "An agent was not added because it did not pass its checks.",
            json!({ "agent_identifier": identifier, "code": code, "check_run_id": check_run_id }),
        )
        .await;
    }

    /// §A6–A10: insert the row, install, inspect, and either make the instance `ready` or refuse it.
    pub(crate) async fn install_new(
        &self,
        spec: NewInstance,
    ) -> Result<InstalledAgent, PackageError> {
        let id = spec.id;
        let mut row = InstanceRow {
            id,
            display_name: spec.display_name.clone(),
            package_name: spec.package_name.clone(),
            package_version: spec.package_version.clone(),
            agent_repository_id: spec.repository.as_ref().map(|(r, _)| *r),
            commit: spec.repository.as_ref().map(|(_, c)| c.clone()),
            // The folder does not exist before §A7; the unique placeholder is replaced there.
            agent_folder_path: format!("pending:{id}"),
            lock_file_path: String::new(),
            audit_result: None,
            authority: Authority::DocumentOnly,
            parsed_declaration_ref: String::new(),
            is_trial_of_own_draft: spec.draft.is_some(),
            status: AgentInstanceStatus::Installing,
            updated_between_runs_at: None,
            installed_at: Timestamp::now(),
            agent_identifier: spec.identifier.clone(),
            agent_draft_id: spec.draft,
            stopped_reason: None,
            stopped_by: None,
            may_do: None,
            plan_hash: None,
            pending_choices: "[]".to_owned(),
            harness: spec.inputs.harness,
            workspace_languages: spec.inputs.languages.clone(),
            interface_locale: spec.inputs.locale.clone(),
        };
        crate::catalogue::validate_instance(&row)?;
        {
            let mut tx = self.pool()?.begin().await?;
            store::insert_instance(&mut tx, &row).await?;
            for choice in &spec.inputs.choices {
                store::put_choice(&mut tx, id, choice).await?;
            }
            tx.commit().await?;
        }
        let _hold = self.hold_hash(&spec.plan.plan_hash);
        let installed = match self.install_twice(&spec).await {
            Ok(installed) => installed,
            Err(error) => {
                row.status = AgentInstanceStatus::Refused;
                row.audit_result = Some(AuditResult::Failed);
                let mut conn = self.conn().await?;
                self.save(&mut conn, &row).await?;
                self.journal(
                    "agent_add_refused",
                    spec.workspace,
                    "An agent could not be installed.",
                    json!({ "agent_identifier": spec.identifier, "code": "PKG_INSTALL_FAILED" }),
                )
                .await;
                return Err(error);
            }
        };
        row.agent_folder_path = installed.folder_path.to_string_lossy().into_owned();
        row.lock_file_path = installed
            .folder_path
            .join("ordin.lock")
            .to_string_lossy()
            .into_owned();
        row.audit_result = Some(AuditResult::Passed);
        row.status = AgentInstanceStatus::Ready;
        apply_inspected(&mut row, &installed.bundle, &installed.inspected);
        let saved = async {
            crate::catalogue::validate_instance(&row)?;
            let mut tx = self.pool()?.begin().await?;
            store::save_instance(&mut tx, &row).await?;
            if let Some(workspace) = spec.workspace {
                store::add_membership(&mut tx, id, workspace).await?;
            }
            tx.commit().await?;
            Ok::<(), PackageError>(())
        }
        .await;
        if let Err(error) = saved {
            self.cleanup(id, true).await;
            return Err(error);
        }
        self.journal(
            "agent_added",
            spec.workspace,
            "An agent was added.",
            json!({
                "agent_instance_id": id.to_string(),
                "agent_identifier": spec.identifier,
                "lock_file_sha256": installed.bundle.manifest.lockfile_sha256,
                "check_run_id": spec.check_run_id,
                "executable_parts": installed.inspected.decisions,
            }),
        )
        .await;
        // A bundled instance names no workspace, so no `Added` notice is sent (PKG-REC-2 step 6).
        if let Some(workspace_id) = spec.workspace {
            self.notify(crate::types::AgentNotice::Added {
                agent_instance_id: id,
                workspace_id,
            });
        }
        self.prune().await;
        self.get(id).await
    }

    /// §A10: one retry after removing the first attempt's folder.
    async fn install_twice(&self, spec: &NewInstance) -> Result<Installed, PackageError> {
        let mut last = String::new();
        for _ in 0..2 {
            match self.install_once(spec).await {
                Ok(installed) => return Ok(installed),
                Err(step) => {
                    last = step.error.code.to_owned();
                    self.cleanup(spec.id, step.reached_installer).await;
                }
            }
        }
        Err(PackageError::install_failed(last))
    }

    /// §A7–A8, one attempt.
    async fn install_once(&self, spec: &NewInstance) -> Result<Installed, StepError> {
        let ports = &self.inner.ports;
        let before = |error: String| StepError {
            error: PackageError::install_failed(error),
            reached_installer: false,
        };
        let folder = ports
            .folders
            .create(spec.id)
            .await
            .map_err(|e| before(e.to_string()))?;
        let bundle = ports
            .resolver
            .build(&spec.request)
            .await
            .map_err(|e| before(e.to_string()))?;
        ports
            .installers
            .install(&InstallRequest {
                agent_instance_id: spec.id,
                harness: spec.inputs.harness,
                plan_hash: bundle.plan.plan_hash.clone(),
            })
            .await
            .map_err(|e| StepError {
                error: PackageError::install_failed(e.to_string()),
                reached_installer: true,
            })?;
        let workspaces: Vec<WorkspaceId> = spec.workspace.into_iter().collect();
        let inspected = self
            .inspect_and_decide(spec.id, &folder.folder_path, &bundle, Some(&workspaces))
            .await
            .map_err(|error| StepError {
                error,
                reached_installer: true,
            })?;
        Ok(Installed {
            folder_path: folder.folder_path,
            bundle,
            inspected,
        })
    }

    /// §A8: inspect the installed folder, store the workspaces with the inspector when given, and
    /// decide the complete executable-part list the install check stored (SA-CLOSE-01).
    pub(crate) async fn inspect_and_decide(
        &self,
        id: AgentInstanceId,
        folder: &std::path::Path,
        bundle: &Bundle,
        workspaces: Option<&[WorkspaceId]>,
    ) -> Result<Inspected, PackageError> {
        let ports = &self.inner.ports;
        let request = self.inspect_request(id, folder, bundle).await?;
        let outcome = ports
            .inspector
            .inspect(&request)
            .await
            .map_err(|e| PackageError::install_failed(e.to_string()))?;
        let parts: Vec<ExecutablePart> = ports
            .checker
            .executable_parts(id)
            .await
            .map_err(|e| PackageError::install_failed(e.to_string()))?;
        let decisions = ports
            .checker
            .decide_executable_parts(id, &parts)
            .await
            .map_err(|e| PackageError::install_failed(e.to_string()))?;
        if let Some(workspaces) = workspaces {
            ports
                .inspector
                .set_workspaces(id, workspaces.to_vec())
                .await
                .map_err(|e| PackageError::install_failed(e.to_string()))?;
        }
        Ok(Inspected { outcome, decisions })
    }

    /// §A10: undo a failed attempt; a failure here is logged, never raised.
    pub(crate) async fn cleanup(&self, id: AgentInstanceId, reached_installer: bool) {
        if reached_installer {
            if let Err(error) = self.inner.ports.installers.uninstall(id).await {
                tracing::warn!(component = "package_catalogue", agent_instance_id = %id, code = error.code(), "agent.cleanup_failed");
            }
        }
        if let Err(error) = self.inner.ports.folders.remove(id).await {
            tracing::warn!(component = "package_catalogue", agent_instance_id = %id, code = %error, "agent.cleanup_failed");
        }
    }

    /// §G: install the person's own draft as a trial.
    pub async fn add_trial(&self, input: TrialInput) -> Result<InstalledAgent, PackageError> {
        let _serial = self.inner.add_lock.lock().await;
        let existing = {
            let mut conn = self.conn().await?;
            store::trial_of_draft(&mut conn, input.agent_draft_id).await?
        };
        if let Some(existing) = existing {
            self.remove_instance(existing.id).await?;
        }
        let ports = &self.inner.ports;
        let source = ports
            .catalogue
            .draft_source(input.agent_repository_id, input.worktree.clone())
            .await
            .map_err(|e| catalogue_failure(&e))?;
        let scope = ports
            .catalogue
            .source_scope(RepositoryRef::Repository(input.agent_repository_id))
            .await
            .map_err(|e| catalogue_failure(&e))?;
        let inputs = PlanInputs {
            languages: input.workspace_languages.clone(),
            locale: input.interface_locale.clone(),
            harness: self.inner.harness.default_harness(),
            choices: Vec::new(),
        };
        let mut request = resolve_request(scope.clone(), &input.agent, None, &inputs);
        request.allow_draft_root = true;
        request.source = Some(SourceOverride(source.clone()));
        let id = AgentInstanceId::new();
        let plan =
            ports
                .resolver
                .resolve(&request)
                .await
                .map_err(|e| match map_resolve_error(&e) {
                    Failure::Err(error) => error,
                    Failure::PackMissing(_) | Failure::Unsafe(_) => {
                        PackageError::agent_not_offered("the draft cannot be built")
                    }
                })?;
        let report = ports
            .checker
            .check_for_install(&InstallCheck {
                plan: plan.clone(),
                scope: scope.clone(),
                agent_instance_id: Some(id),
                source: request.source.clone(),
            })
            .await
            .map_err(|e| PackageError::check_unavailable(e.to_string()))?;
        let check_run_id = report.report.check_run_id.to_string();
        match report.verdict {
            InstallVerdict::Clean => {}
            InstallVerdict::Failed => {
                return Err(PackageError::agent_not_offered(
                    "the draft fails its checks",
                ));
            }
            InstallVerdict::NeedsItApproval { .. } => return Err(PackageError::trial_needs_trust()),
        }
        let display_name = match source
            .resolve(&scope, PartKind::WorkspaceProfile, &input.agent, None)
            .await
        {
            Ok(Resolution::Found(found)) => found.part.name,
            _ => plan.root.name.clone(),
        };
        let spec = NewInstance {
            id,
            workspace: Some(input.workspace_id),
            display_name,
            package_name: slug_from_id(&input.agent),
            package_version: plan.root.version.clone(),
            repository: None,
            identifier: input.agent.clone(),
            draft: Some(input.agent_draft_id),
            inputs,
            request,
            plan,
            check_run_id,
        };
        self.install_new(spec).await
    }
}

/// §A8: the inspector's findings onto the row.
pub(crate) fn apply_inspected(row: &mut InstanceRow, bundle: &Bundle, inspected: &Inspected) {
    row.authority = inspected.outcome.model.authority;
    row.parsed_declaration_ref = inspected.outcome.parsed_declaration_ref.clone();
    row.may_do = serde_json::to_string(&inspected.outcome.may_do).ok();
    row.plan_hash = Some(bundle.plan.plan_hash.clone());
    row.pending_choices = serde_json::to_string(&bundle.plan.selections.pending_choices)
        .unwrap_or_else(|_| "[]".to_owned());
}

fn stored_choices(choices: &[crate::types::PackChoice]) -> Result<Vec<StoredChoice>, PackageError> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(choices.len());
    for choice in choices {
        if !seen.insert(choice.rule_index) {
            return Err(PackageError::choice_not_allowed("a rule is chosen twice"));
        }
        out.push(StoredChoice {
            rule_index: choice.rule_index,
            packs: choice.packs.clone(),
            source: ChoiceSource::PersonChange,
        });
    }
    Ok(out)
}

pub(crate) fn part_names(parts: &[ExecutablePart]) -> Vec<String> {
    parts.iter().map(|p| p.part_name.clone()).collect()
}
