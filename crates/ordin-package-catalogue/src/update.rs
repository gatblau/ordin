//! Updates between runs (PackageCatalogue.md §C), withdrawals (§D) and the background loop `run`.

use ordin_catalogue::CatalogueNotice;
use ordin_checker::{InstallCheck, InstallVerdict};
use ordin_installers::InstallRequest;
use ordin_package_inspector::{compare_may_do, may_do, MayDo, MayDoChange};
use ordin_resolver::model::{PartRef, RepositoryRef, SourceScope};
use ordin_resolver::{Replacement, ResolveRequest};
use ordin_types::domain::agents::{AgentInstanceStatus, StoppedBy, WithdrawnListEntry};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, AgentRepositoryId, WithdrawnListEntryId};
use serde_json::json;
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::add::apply_inspected;
use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::request::{resolve_request, PlanInputs, RepoIndex};
use crate::store::{self, InstanceRow, PendingRow};
use crate::types::{AgentNotice, InstalledAgent};

/// How a candidate bundle was received (§C1–C3).
#[derive(Debug, PartialEq, Eq)]
enum Candidate {
    /// The candidate's plan hash equals the installed one.
    Unchanged,
    /// The Checker's verdict is `Failed`.
    CheckFailed,
    /// An update that changes nothing the agent may do or read; applied now or when the run ends.
    Scheduled,
    /// The person is asked (`update_awaiting_choice`).
    AwaitingChoice,
    /// A provider could not be read; nothing changed.
    Unavailable,
}

/// How an update is applied (§C3, §C4, §C5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Apply {
    /// Between runs, nothing was asked: `updated_between_runs_at` is set.
    Silent,
    /// The person accepted a choice.
    Accepted,
    /// The person accepted the fixed version of a stopped agent: `refused` becomes `ready`.
    Resumed,
}

impl PackageCatalogue {
    /// The notices loop (§C, §D): reads the Catalogue Service's notices until `shutdown` fires.
    pub async fn run(&self, shutdown: CancellationToken) -> Result<(), PackageError> {
        let mut notices = self.inner.ports.catalogue.subscribe();
        // A restart drops an update that was waiting for a run to end; sweep again, as a notice would.
        self.sweep_candidates(&shutdown).await;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return Ok(()),
                received = notices.recv() => match received {
                    Ok(CatalogueNotice::RepositoryUpdated { agent_repository_id, newly_withdrawn, .. }) => {
                        self.on_repository_updated(agent_repository_id, &newly_withdrawn, &shutdown).await;
                    }
                    Ok(CatalogueNotice::RepositoryDropped { agent_repository_id }) => {
                        // R-72: its agents keep running from their folders; no update is offered again.
                        self.inner
                            .dropped
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .insert(agent_repository_id);
                        tracing::info!(component = "package_catalogue", repository_id = %agent_repository_id, "agent.repository_dropped");
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::warn!(component = "package_catalogue", skipped, "agent.notices_lagged");
                    }
                    Err(RecvError::Closed) => return Ok(()),
                },
            }
        }
    }

    /// Considers candidate updates for every repository that holds a `ready` instance (§C1).
    async fn sweep_candidates(&self, shutdown: &CancellationToken) {
        let rows = match self.conn().await {
            Ok(mut conn) => store::ready_instances(&mut conn).await,
            Err(error) => Err(error),
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    component = "package_catalogue",
                    code = error.code,
                    "agent.update_failed"
                );
                return;
            }
        };
        let repositories: std::collections::BTreeSet<AgentRepositoryId> =
            rows.iter().filter_map(|r| r.agent_repository_id).collect();
        for repository in repositories {
            self.on_repository_updated(repository, &[], shutdown).await;
        }
    }

    pub(crate) async fn on_repository_updated(
        &self,
        repository: AgentRepositoryId,
        newly_withdrawn: &[WithdrawnListEntryId],
        shutdown: &CancellationToken,
    ) {
        let dropped = self
            .inner
            .dropped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&repository);
        if dropped {
            return;
        }
        if !newly_withdrawn.is_empty() {
            if let Err(error) = self
                .handle_withdrawals(repository, newly_withdrawn, shutdown)
                .await
            {
                tracing::warn!(
                    component = "package_catalogue",
                    code = error.code,
                    "agent.withdrawal_failed"
                );
            }
        }
        let rows = match self.conn().await {
            Ok(mut conn) => store::updatable_instances(&mut conn, repository).await,
            Err(error) => Err(error),
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    component = "package_catalogue",
                    code = error.code,
                    "agent.update_failed"
                );
                return;
            }
        };
        for row in rows {
            let lock = self.lock_for(row.id);
            let _held = lock.lock().await;
            let fresh = match self.live_row(row.id).await {
                Ok(fresh) => fresh,
                Err(_) => continue,
            };
            let done = match (fresh.status, fresh.stopped_by) {
                (AgentInstanceStatus::Ready, _) => self.process_update(&fresh, shutdown).await,
                (AgentInstanceStatus::Refused, Some(StoppedBy::Withdrawal)) => {
                    self.process_fixed_version(&fresh).await
                }
                _ => Ok(()),
            };
            if let Err(error) = done {
                tracing::warn!(component = "package_catalogue", agent_instance_id = %fresh.id, code = error.code, "agent.update_failed");
            }
        }
    }

    async fn plan_inputs(&self, row: &InstanceRow) -> Result<PlanInputs, PackageError> {
        let mut conn = self.conn().await?;
        Ok(PlanInputs {
            languages: row.workspace_languages.clone(),
            locale: row.interface_locale.clone(),
            harness: row.harness,
            choices: store::choices_of(&mut conn, row.id).await?,
        })
    }

    /// §C1–C3 for one `ready` instance.
    async fn process_update(
        &self,
        row: &InstanceRow,
        shutdown: &CancellationToken,
    ) -> Result<(), PackageError> {
        let scope = self.scope_of(row).await?;
        let inputs = self.plan_inputs(row).await?;
        let request = resolve_request(scope.clone(), &row.agent_identifier, None, &inputs);
        match self
            .consider_candidate(row, &request, &scope, shutdown)
            .await
        {
            Candidate::CheckFailed => {
                let workspace = self.first_workspace(row.id).await;
                self.journal(
                    "agent_update_refused",
                    workspace,
                    "An update was not applied because it did not pass its checks.",
                    json!({ "agent_instance_id": row.id.to_string() }),
                )
                .await;
                tracing::info!(component = "package_catalogue", agent_instance_id = %row.id, kind = "refused", "agent.update");
            }
            Candidate::Unchanged
            | Candidate::Scheduled
            | Candidate::AwaitingChoice
            | Candidate::Unavailable => {}
        }
        Ok(())
    }

    /// Resolve, build, check and inspect a candidate for `row`, then schedule it or ask (§C1–C3).
    async fn consider_candidate(
        &self,
        row: &InstanceRow,
        request: &ResolveRequest,
        scope: &SourceScope,
        shutdown: &CancellationToken,
    ) -> Candidate {
        let ports = &self.inner.ports;
        let id = row.id;
        let plan = match ports.resolver.resolve(request).await {
            Ok(plan) => plan,
            Err(error) => {
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, code = error.code(), "agent.update_unavailable");
                return Candidate::Unavailable;
            }
        };
        if row.plan_hash.as_deref() == Some(plan.plan_hash.as_str()) {
            return Candidate::Unchanged;
        }
        let candidate = match ports.resolver.build(request).await {
            Ok(bundle) => bundle,
            Err(error) => {
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, code = error.code(), "agent.update_unavailable");
                return Candidate::Unavailable;
            }
        };
        let hold = self.hold_hash(&candidate.plan.plan_hash);
        let verdict = match ports
            .checker
            .check_for_install(&InstallCheck {
                plan: candidate.plan.clone(),
                scope: scope.clone(),
                agent_instance_id: Some(id),
                source: None,
            })
            .await
        {
            Ok(report) => report.verdict,
            Err(error) => {
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, code = error.code(), "agent.update_unavailable");
                return Candidate::Unavailable;
            }
        };
        // PKG-REC-3: the person accepted a bundled agent's new version with the application's, so
        // it is never asked about; a candidate that needs IT approval is kept out as `Failed` is.
        // spec: ordin-desktop/components/PackageCatalogue#PKG-REC-3
        let bundled = crate::types::is_bundled(id);
        let needs_it = match verdict {
            InstallVerdict::Failed => return Candidate::CheckFailed,
            InstallVerdict::NeedsItApproval { .. } if bundled => return Candidate::CheckFailed,
            InstallVerdict::NeedsItApproval { parts } => {
                // `PendingUpdate` has no field for the parts that need approval (§C2); their names
                // are logged and the person is asked.
                let names: Vec<&str> = parts.iter().map(|p| p.part_name.as_str()).collect();
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, parts = ?names, "agent.update_needs_it");
                true
            }
            InstallVerdict::Clean => false,
        };
        let installed: Option<MayDo> = row
            .may_do
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok());
        let request = match self
            .inspect_request(id, std::path::Path::new(&row.agent_folder_path), &candidate)
            .await
        {
            Ok(request) => request,
            Err(_) => return Candidate::Unavailable,
        };
        let model = match ports.inspector.inspect_candidate(&request).await {
            Ok(model) => model,
            Err(error) => {
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, code = error.code(), "agent.update_unavailable");
                return Candidate::Unavailable;
            }
        };
        let changes: Vec<MayDoChange> = match &installed {
            Some(installed) => compare_may_do(installed, &may_do(&model)),
            None => Vec::new(),
        };
        // An unreadable stored summary cannot show that nothing changed, so the person is asked.
        let ask = !bundled && (needs_it || !changes.is_empty() || installed.is_none());
        if ask {
            let pending = PendingRow {
                to_commit: candidate.plan.root.commit.clone(),
                may_do_changes: serde_json::to_string(&changes).unwrap_or_else(|_| "[]".to_owned()),
                candidate_plan_hash: candidate.plan.plan_hash.clone(),
            };
            return match self.mark_awaiting(row, &pending).await {
                Ok(()) => Candidate::AwaitingChoice,
                Err(_) => Candidate::Unavailable,
            };
        }
        if self.inner.activity.running(id) {
            // Nothing changes until the run ends (A-16); the candidate's bundle stays held.
            let this = self.clone();
            let token = shutdown.clone();
            let plan_hash = candidate.plan.plan_hash.clone();
            tokio::spawn(async move {
                let _hold = hold;
                if !this.wait_idle(id, &token).await {
                    return;
                }
                let lock = this.lock_for(id);
                let _held = lock.lock().await;
                let Ok(fresh) = this.live_row(id).await else {
                    return;
                };
                if fresh.status != AgentInstanceStatus::Ready {
                    return;
                }
                if let Err(error) = this.apply_candidate(fresh, &plan_hash, Apply::Silent).await {
                    tracing::warn!(component = "package_catalogue", agent_instance_id = %id, code = error.code, "agent.update_failed");
                }
            });
            return Candidate::Scheduled;
        }
        drop(hold);
        match self
            .apply_candidate(row.clone(), &candidate.plan.plan_hash, Apply::Silent)
            .await
        {
            Ok(_) => Candidate::Scheduled,
            Err(_) => Candidate::Unavailable,
        }
    }

    /// `update_awaiting_choice` with its pending row, and the notice (§C3).
    async fn mark_awaiting(
        &self,
        row: &InstanceRow,
        pending: &PendingRow,
    ) -> Result<(), PackageError> {
        let mut next = row.clone();
        next.status = AgentInstanceStatus::UpdateAwaitingChoice;
        {
            let mut tx = self.pool()?.begin().await?;
            self.save(&mut tx, &next).await?;
            store::put_pending(&mut tx, row.id, pending).await?;
            tx.commit().await?;
        }
        self.notify(AgentNotice::UpdateNeedsChoice {
            agent_instance_id: row.id,
        });
        tracing::info!(component = "package_catalogue", agent_instance_id = %row.id, kind = "needs_choice", to_commit = %pending.to_commit, "agent.update");
        Ok(())
    }

    /// Waits until the instance has no running session; false when the wait cannot continue.
    pub(crate) async fn wait_idle(
        &self,
        id: AgentInstanceId,
        shutdown: &CancellationToken,
    ) -> bool {
        let mut idle = self.inner.activity.idle();
        loop {
            if !self.inner.activity.running(id) {
                return true;
            }
            tokio::select! {
                () = shutdown.cancelled() => return false,
                received = idle.recv() => match received {
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => return false,
                },
            }
        }
    }

    /// §C3 and §C4: install the candidate bundle `plan_hash`, re-inspect, and make the instance `ready`.
    async fn apply_candidate(
        &self,
        mut row: InstanceRow,
        plan_hash: &str,
        mode: Apply,
    ) -> Result<InstalledAgent, PackageError> {
        let ports = &self.inner.ports;
        let id = row.id;
        let bundle = ports
            .resolver
            .open_bundle(plan_hash)
            .map_err(|e| PackageError::install_failed(e.to_string()))?;
        let before = row.clone();
        if mode != Apply::Resumed {
            row.status = AgentInstanceStatus::Updating;
            let mut conn = self.conn().await?;
            self.save(&mut conn, &row).await?;
        }
        let outcome = async {
            ports
                .installers
                .update(&InstallRequest {
                    agent_instance_id: id,
                    harness: row.harness,
                    plan_hash: plan_hash.to_owned(),
                })
                .await
                .map_err(|e| PackageError::install_failed(e.to_string()))?;
            self.inspect_and_decide(
                id,
                std::path::Path::new(&row.agent_folder_path),
                &bundle,
                None,
            )
            .await
        }
        .await;
        let inspected = match outcome {
            Ok(inspected) => inspected,
            Err(error) => {
                let mut conn = self.conn().await?;
                self.save(&mut conn, &before).await?;
                let workspace = self.first_workspace(id).await;
                self.journal(
                    "agent_update_refused",
                    workspace,
                    "An update could not be installed.",
                    json!({ "agent_instance_id": id.to_string(), "code": error.code }),
                )
                .await;
                return Err(error);
            }
        };
        apply_inspected(&mut row, &bundle, &inspected);
        row.package_version = bundle.plan.root.version.clone();
        if row.agent_repository_id.is_some() {
            row.commit = Some(bundle.plan.root.commit.clone());
        }
        row.status = AgentInstanceStatus::Ready;
        row.stopped_reason = None;
        row.stopped_by = None;
        if mode == Apply::Silent {
            row.updated_between_runs_at = Some(Timestamp::now());
        }
        {
            let mut tx = self.pool()?.begin().await?;
            self.save(&mut tx, &row).await?;
            store::delete_pending(&mut tx, id).await?;
            tx.commit().await?;
        }
        let workspace = self.first_workspace(id).await;
        match mode {
            Apply::Silent => {
                self.notify(AgentNotice::UpdatedBetweenRuns {
                    agent_instance_id: id,
                });
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, kind = "silent", to_commit = %bundle.plan.root.commit, "agent.update");
            }
            Apply::Resumed => {
                self.notify(AgentNotice::Resumed {
                    agent_instance_id: id,
                });
                self.journal(
                    "agent_resumed",
                    workspace,
                    "An agent can run again with a fixed version.",
                    json!({ "agent_instance_id": id.to_string() }),
                )
                .await;
            }
            Apply::Accepted => {
                tracing::info!(component = "package_catalogue", agent_instance_id = %id, kind = "accepted", "agent.update");
            }
        }
        self.prune().await;
        self.get(id).await
    }

    // spec: ordin-desktop/components/PackageCatalogue#SA-CLOSE-02
    /// §C5: a fixed version for an agent a withdrawal stopped.
    async fn process_fixed_version(&self, row: &InstanceRow) -> Result<(), PackageError> {
        let ports = &self.inner.ports;
        let id = row.id;
        let scope = self.scope_of(row).await?;
        let inputs = self.plan_inputs(row).await?;
        let request = resolve_request(scope.clone(), &row.agent_identifier, None, &inputs);
        // A `ResolveError` (the part is still withdrawn, or no replacement resolves): nothing changes.
        let Ok(candidate) = ports.resolver.build(&request).await else {
            return Ok(());
        };
        let _hold = self.hold_hash(&candidate.plan.plan_hash);
        let report = match ports
            .checker
            .check_for_install(&InstallCheck {
                plan: candidate.plan.clone(),
                scope,
                agent_instance_id: Some(id),
                source: None,
            })
            .await
        {
            Ok(report) => report,
            Err(_) => return Ok(()),
        };
        if !matches!(report.verdict, InstallVerdict::Clean) {
            return Ok(());
        }
        let inspect = self
            .inspect_request(id, std::path::Path::new(&row.agent_folder_path), &candidate)
            .await?;
        let model = ports
            .inspector
            .inspect_candidate(&inspect)
            .await
            .map_err(|e| PackageError::install_failed(e.to_string()))?;
        let installed: Option<MayDo> = row
            .may_do
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok());
        let changes = match &installed {
            Some(installed) => compare_may_do(installed, &may_do(&model)),
            None => Vec::new(),
        };
        let pending = PendingRow {
            to_commit: candidate.plan.root.commit.clone(),
            may_do_changes: serde_json::to_string(&changes).unwrap_or_else(|_| "[]".to_owned()),
            candidate_plan_hash: candidate.plan.plan_hash.clone(),
        };
        {
            let mut conn = self.conn().await?;
            store::put_pending(&mut conn, id, &pending).await?;
        }
        // The status stays `refused`.
        self.notify(AgentNotice::UpdateNeedsChoice {
            agent_instance_id: id,
        });
        tracing::info!(component = "package_catalogue", agent_instance_id = %id, kind = "needs_choice", to_commit = %pending.to_commit, "agent.update");
        Ok(())
    }

    /// §C4: the person accepts an update that waits.
    pub async fn accept_update(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError> {
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        let (row, pending) = self.waiting_update(id).await?;
        if row.status == AgentInstanceStatus::UpdateAwaitingChoice {
            // Between runs: wait until no session runs; dropping this call leaves the state as it was.
            let never = CancellationToken::new();
            if !self.wait_idle(id, &never).await {
                return Err(PackageError::instance_not_ready(
                    "the instance is still running",
                ));
            }
        }
        let mode = if row.status == AgentInstanceStatus::Refused {
            Apply::Resumed
        } else {
            Apply::Accepted
        };
        self.apply_candidate(row, &pending.candidate_plan_hash, mode)
            .await
    }

    /// §C4: the person declines it.
    pub async fn decline_update(
        &self,
        id: AgentInstanceId,
    ) -> Result<InstalledAgent, PackageError> {
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        let (mut row, _) = self.waiting_update(id).await?;
        if row.status == AgentInstanceStatus::UpdateAwaitingChoice {
            row.status = AgentInstanceStatus::Ready;
        }
        {
            let mut tx = self.pool()?.begin().await?;
            self.save(&mut tx, &row).await?;
            store::delete_pending(&mut tx, id).await?;
            tx.commit().await?;
        }
        self.prune().await;
        self.get(id).await
    }

    /// The instance and its pending row; `PKG_NO_UPDATE` when none waits.
    async fn waiting_update(
        &self,
        id: AgentInstanceId,
    ) -> Result<(InstanceRow, PendingRow), PackageError> {
        let mut conn = self.conn().await?;
        let row = store::get_instance(&mut conn, id)
            .await?
            .filter(|r| r.status != AgentInstanceStatus::Removed)
            .ok_or_else(PackageError::instance_unknown)?;
        let waiting = matches!(
            row.status,
            AgentInstanceStatus::UpdateAwaitingChoice | AgentInstanceStatus::Refused
        );
        let pending = store::pending_of(&mut conn, id).await?;
        match pending {
            Some(pending) if waiting => Ok((row, pending)),
            _ => Err(PackageError::no_update()),
        }
    }

    /// §D: each newly withdrawn entry, for the instances whose lock file names the part.
    async fn handle_withdrawals(
        &self,
        repository: AgentRepositoryId,
        newly_withdrawn: &[WithdrawnListEntryId],
        shutdown: &CancellationToken,
    ) -> Result<(), PackageError> {
        let ports = &self.inner.ports;
        let scope = ports
            .catalogue
            .source_scope(RepositoryRef::Repository(repository))
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
        let entries: Vec<WithdrawnListEntry> = ports
            .catalogue
            .withdrawn(&scope)
            .await
            .map_err(|_| {
                PackageError::library_unavailable(false, "the withdrawn list is unreadable")
            })?
            .into_iter()
            .filter(|e| newly_withdrawn.contains(&e.id))
            .collect();
        if entries.is_empty() {
            return Ok(());
        }
        let rows = {
            let mut conn = self.conn().await?;
            store::ready_instances(&mut conn).await?
        };
        let repos = self.repo_index().await.unwrap_or_default();
        for row in rows {
            let lock = self.lock_for(row.id);
            let _held = lock.lock().await;
            let Ok(fresh) = self.live_row(row.id).await else {
                continue;
            };
            if fresh.status != AgentInstanceStatus::Ready {
                continue;
            }
            let Some(hash) = fresh.plan_hash.clone() else {
                continue;
            };
            let Ok(bundle) = ports.resolver.open_bundle(&hash) else {
                continue;
            };
            let parts: Vec<&PartRef> = std::iter::once(&bundle.lock_file.composition)
                .chain(bundle.lock_file.artefacts.iter())
                .collect();
            let hit = entries.iter().find_map(|entry| {
                parts
                    .iter()
                    .find(|p| {
                        p.kind == entry.part_kind
                            && p.id == entry.part_identifier
                            && entry.part_version.as_deref().is_none_or(|v| v == p.version)
                    })
                    .map(|p| (entry, (*p).clone()))
            });
            let Some((entry, part)) = hit else {
                continue;
            };
            if self.try_replacement(&fresh, entry, shutdown).await {
                continue;
            }
            if let Err(error) = self.stop_for_withdrawal(&fresh, entry, &part, &repos).await {
                tracing::warn!(component = "package_catalogue", agent_instance_id = %fresh.id, code = error.code, "agent.withdrawal_failed");
            }
        }
        Ok(())
    }

    /// §D1: a replacement is named, so the agent is re-planned with it, checked and updated.
    async fn try_replacement(
        &self,
        row: &InstanceRow,
        entry: &WithdrawnListEntry,
        shutdown: &CancellationToken,
    ) -> bool {
        let Some(replacement) = &entry.replacement_identifier else {
            return false;
        };
        let Ok(scope) = self.scope_of(row).await else {
            return false;
        };
        let Ok(inputs) = self.plan_inputs(row).await else {
            return false;
        };
        let mut request = resolve_request(scope.clone(), &row.agent_identifier, None, &inputs);
        request.replacements = vec![Replacement {
            withdrawn_id: entry.part_identifier.clone(),
            withdrawn_kind: entry.part_kind,
            replacement_id: replacement.clone(),
            replacement_version: entry.replacement_version.clone(),
        }];
        matches!(
            self.consider_candidate(row, &request, &scope, shutdown)
                .await,
            Candidate::Scheduled | Candidate::AwaitingChoice | Candidate::Unchanged
        )
    }

    /// §D2: no replacement, or it fails: the instance stops.
    async fn stop_for_withdrawal(
        &self,
        row: &InstanceRow,
        entry: &WithdrawnListEntry,
        part: &PartRef,
        repos: &RepoIndex,
    ) -> Result<(), PackageError> {
        let repository = repos
            .display
            .get(&part.repository)
            .cloned()
            .unwrap_or_else(|| "its repository".to_owned());
        let reason = format!(
            "{} stopped: {} was withdrawn by {}.",
            row.display_name, part.name, repository
        );
        let mut next = row.clone();
        next.status = AgentInstanceStatus::Refused;
        next.stopped_by = Some(StoppedBy::Withdrawal);
        next.stopped_reason = Some(reason.clone());
        {
            let mut tx = self.pool()?.begin().await?;
            self.save(&mut tx, &next).await?;
            store::delete_pending(&mut tx, row.id).await?;
            tx.commit().await?;
        }
        self.notify(AgentNotice::Stopped {
            agent_instance_id: row.id,
            reason,
        });
        let workspace = self.first_workspace(row.id).await;
        self.journal(
            "agent_stopped",
            workspace,
            "An agent stopped because a part it uses was withdrawn.",
            json!({ "agent_instance_id": row.id.to_string(), "entry_id": entry.id.to_string() }),
        )
        .await;
        tracing::info!(component = "package_catalogue", agent_instance_id = %row.id, entry_id = %entry.id, "agent.stopped");
        Ok(())
    }
}
