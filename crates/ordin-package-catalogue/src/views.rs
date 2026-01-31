//! Reading instances (PackageCatalogue.md §H and `list`) and the lookups of the Ordin Host and the
//! Knowledge Service (§J, §K, §M, §N).

use ordin_catalogue::{LibraryQuery, PartStateLabel};
use ordin_package_inspector::{MayDo, MayDoChange, MayDoChangeKind};
use ordin_resolver::model::{PartKind, PendingPackChoice, SourceScope};
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::domain::agents::AgentRepository;
use ordin_types::{AgentInstanceId, AgentRepositoryId, WorkspaceId};
use serde::Deserialize;

use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::request::{home_of, RepoIndex};
use crate::store::{self, InstanceRow};
use crate::types::{
    AgentFound, AgentMatch, AgentRepositoryRef, InstalledAgent, PendingChoice, PendingChoiceOption,
    PendingUpdate, TaskForm, WorkspaceAgent,
};

/// The stored form of a `MayDoChange` (`agent_pending_updates.may_do_changes`).
#[derive(Debug, Deserialize)]
struct StoredChange {
    kind: MayDoChangeKind,
    name: String,
}

/// The Fluent message id the Package Inspector gives each kind (`may-do-<kind in kebab-case>`).
fn message_id(kind: MayDoChangeKind) -> &'static str {
    match kind {
        MayDoChangeKind::ToolAdded => "may-do-tool-added",
        MayDoChangeKind::ToolRemoved => "may-do-tool-removed",
        MayDoChangeKind::ReadAdded => "may-do-read-added",
        MayDoChangeKind::ReadRemoved => "may-do-read-removed",
        MayDoChangeKind::ConnectionAdded => "may-do-connection-added",
        MayDoChangeKind::ConnectionRemoved => "may-do-connection-removed",
        MayDoChangeKind::PolicyChanged => "may-do-policy-changed",
        MayDoChangeKind::DeclarationChanged => "may-do-declaration-changed",
        MayDoChangeKind::AuthorityChanged => "may-do-authority-changed",
    }
}

pub(crate) fn decode_changes(text: &str) -> Result<Vec<MayDoChange>, PackageError> {
    let stored: Vec<StoredChange> = serde_json::from_str(text)
        .map_err(|_| PackageError::store(false, "a stored update value is not readable"))?;
    Ok(stored
        .into_iter()
        .map(|c| MayDoChange {
            kind: c.kind,
            name: c.name,
            message_id: message_id(c.kind),
        })
        .collect())
}

impl PackageCatalogue {
    /// The row of an instance that exists and is not `removed`.
    pub(crate) async fn live_row(&self, id: AgentInstanceId) -> Result<InstanceRow, PackageError> {
        let mut conn = self.conn().await?;
        match store::get_instance(&mut conn, id).await? {
            Some(row) if row.status != AgentInstanceStatus::Removed => Ok(row),
            _ => Err(PackageError::instance_unknown()),
        }
    }

    /// §H: one instance by id, removed instances included.
    pub async fn get(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError> {
        let row = {
            let mut conn = self.conn().await?;
            store::get_instance(&mut conn, id)
                .await?
                .ok_or_else(PackageError::instance_unknown)?
        };
        self.installed_agent(row, None).await
    }

    /// The agents on this computer that are not removed, for one workspace or for all.
    pub async fn list(
        &self,
        workspace_id: Option<WorkspaceId>,
    ) -> Result<Vec<InstalledAgent>, PackageError> {
        let rows = {
            let mut conn = self.conn().await?;
            match workspace_id {
                Some(workspace) => store::instances_in_workspace(&mut conn, workspace).await?,
                None => store::live_instances(&mut conn).await?,
            }
        };
        let repos = self.repo_index().await.unwrap_or_default();
        let mut out = Vec::with_capacity(rows.len());
        // spec: ordin-desktop/components/PackageCatalogue#PKG-REC-3
        for row in rows.into_iter().filter(|r| !crate::types::is_bundled(r.id)) {
            out.push(self.installed_agent(row, Some(&repos)).await?);
        }
        Ok(out)
    }

    pub(crate) async fn installed_agent(
        &self,
        row: InstanceRow,
        repos: Option<&RepoIndex>,
    ) -> Result<InstalledAgent, PackageError> {
        let (workspaces, pending) = {
            let mut conn = self.conn().await?;
            (
                store::workspaces_of(&mut conn, row.id).await?,
                store::pending_of(&mut conn, row.id).await?,
            )
        };
        let update = match pending {
            Some(p) => Some(PendingUpdate {
                to_commit: p.to_commit,
                may_do_changes: decode_changes(&p.may_do_changes)?,
            }),
            None => None,
        };
        let (mut tasks, mut uses, mut task_forms) = (Vec::new(), Vec::new(), Vec::new());
        let installed = matches!(
            row.status,
            AgentInstanceStatus::Ready
                | AgentInstanceStatus::Updating
                | AgentInstanceStatus::UpdateAwaitingChoice
        );
        if installed {
            match self.inner.ports.inspector.model(row.id).await {
                Ok(model) => {
                    tasks = model.tasks.iter().map(|t| t.name.clone()).collect();
                    // spec: ordin-desktop/components/PackageCatalogue#PKG-TF-1
                    task_forms = model.tasks.iter().map(TaskForm::of).collect();
                }
                Err(error) => {
                    tracing::warn!(
                        component = "package_catalogue",
                        agent_instance_id = row.id.to_string().as_str(),
                        code = error.code(),
                        "agent.tasks_unreadable"
                    );
                }
            }
            if let Some(hash) = &row.plan_hash {
                match self.inner.ports.resolver.open_bundle(hash) {
                    Ok(bundle) => {
                        let owned;
                        let index = match repos {
                            Some(index) => index,
                            None => {
                                owned = self.repo_index().await.unwrap_or_default();
                                &owned
                            }
                        };
                        uses = self.uses_of(&bundle.plan, index).await;
                    }
                    Err(error) => {
                        tracing::warn!(
                            component = "package_catalogue",
                            agent_instance_id = row.id.to_string().as_str(),
                            code = error.code(),
                            "agent.uses_unreadable"
                        );
                    }
                }
            }
        }
        Ok(InstalledAgent {
            instance: row.to_instance(),
            workspaces,
            tasks,
            uses,
            update,
            task_forms,
        })
    }

    // spec: ordin-desktop/components/PackageCatalogue#SA-CLOSE-03
    /// §M: the source scope an installed instance's parts resolve in (SA-CLOSE-03).
    pub async fn source_scope(
        &self,
        instance: AgentInstanceId,
    ) -> Result<SourceScope, PackageError> {
        let row = self.live_row(instance).await?;
        if row.is_trial_of_own_draft {
            return Err(PackageError::trial_no_scope());
        }
        self.scope_of(&row).await
    }

    /// §M.2: the Catalogue Service's scope of the row's home repository.
    pub(crate) async fn scope_of(&self, row: &InstanceRow) -> Result<SourceScope, PackageError> {
        self.inner
            .ports
            .catalogue
            .source_scope(home_of(row.agent_repository_id))
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))
    }

    /// §J1: workflow file format 3.
    pub async fn match_workflow_agent(
        &self,
        workspace_id: WorkspaceId,
        repository: &AgentRepositoryRef,
        id: &str,
        version: &str,
    ) -> Result<AgentMatch, PackageError> {
        let found: Option<AgentRepository> = self
            .inner
            .ports
            .catalogue
            .repository_by_ref(repository)
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
        let Some(found) = found else {
            return Ok(AgentMatch::Missing);
        };
        let rows = self.workspace_rows(workspace_id).await?;
        let serving: Vec<&InstanceRow> = rows
            .iter()
            .filter(|r| r.agent_repository_id == Some(found.id) && r.agent_identifier == id)
            .collect();
        if let Some(same) = serving.iter().find(|r| r.package_version == version) {
            return Ok(AgentMatch::Present {
                agent_instance_id: same.id,
                display_name: same.display_name.clone(),
            });
        }
        Ok(match serving.first() {
            Some(other) => AgentMatch::OtherVersion {
                agent_instance_id: other.id,
                display_name: other.display_name.clone(),
                installed_version: other.package_version.clone(),
            },
            None => AgentMatch::Missing,
        })
    }

    /// §J2: workflow file formats 1 and 2.
    pub async fn match_by_name(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
    ) -> Result<AgentMatch, PackageError> {
        let wanted = name.trim().to_lowercase();
        let rows = self.workspace_rows(workspace_id).await?;
        let mut named = rows
            .iter()
            .filter(|r| r.display_name.trim().to_lowercase() == wanted);
        let first = named.next();
        if named.next().is_some() {
            return Err(PackageError::name_ambiguous());
        }
        Ok(match first {
            Some(row) => AgentMatch::Present {
                agent_instance_id: row.id,
                display_name: row.display_name.clone(),
            },
            None => AgentMatch::Missing,
        })
    }

    async fn workspace_rows(
        &self,
        workspace: WorkspaceId,
    ) -> Result<Vec<InstanceRow>, PackageError> {
        let mut conn = self.conn().await?;
        store::instances_in_workspace(&mut conn, workspace).await
    }

    /// §J3: what a workflow file format 3 names the instance by.
    pub async fn agent_identity(
        &self,
        agent_instance_id: AgentInstanceId,
    ) -> Result<(AgentRepositoryRef, String, String, String), PackageError> {
        let row = self.live_row(agent_instance_id).await?;
        let Some(repository) = row
            .agent_repository_id
            .filter(|_| !row.is_trial_of_own_draft)
        else {
            return Err(PackageError::trial_not_nameable());
        };
        let reference = self
            .inner
            .ports
            .catalogue
            .repository_ref(repository)
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
        Ok((
            reference,
            row.agent_identifier,
            row.package_version,
            row.display_name,
        ))
    }

    /// §J4: library agents that fit a need in words, at most 10, best first.
    pub async fn find_agents(
        &self,
        workspace_id: WorkspaceId,
        need: &str,
    ) -> Result<Vec<AgentFound>, PackageError> {
        let need = need.trim();
        if need.is_empty() || need.chars().count() > 500 {
            return Err(PackageError::need_invalid());
        }
        let page = self
            .inner
            .ports
            .catalogue
            .search(LibraryQuery {
                text: Some(need.to_owned()),
                kinds: Vec::new(),
                repositories: Vec::new(),
                area: None,
                audience: None,
                publisher: None,
                include_not_for_new: false,
                cursor: None,
            })
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
        let rows = self.workspace_rows(workspace_id).await?;
        Ok(page
            .items
            .into_iter()
            // PKG-OWN-1: ordin's own agents are never offered.
            .filter(|entry| {
                !crate::types::is_own_agent(
                    entry.part.part.agent_repository_id,
                    &entry.part.part.identifier,
                )
            })
            .take(10)
            .map(|entry| {
                let part = entry.part.part;
                let in_workspace = rows
                    .iter()
                    .find(|r| {
                        r.agent_repository_id == Some(part.agent_repository_id)
                            && r.agent_identifier == part.identifier
                    })
                    .map(|r| r.id);
                AgentFound {
                    agent: part.id,
                    agent_repository_id: part.agent_repository_id,
                    identifier: part.identifier,
                    name: part.name,
                    summary: entry.part.summary,
                    from: entry.from,
                    in_workspace,
                }
            })
            .collect())
    }

    /// §J5: confirms an agent may be proposed.
    pub async fn check_proposal(
        &self,
        agent_repository_id: AgentRepositoryId,
        agent: &str,
    ) -> Result<(), PackageError> {
        let view = self
            .inner
            .ports
            .catalogue
            .find_part(agent_repository_id, PartKind::WorkspaceProfile, agent, None)
            .await
            .map_err(|e| match e.code {
                "CAT_PART_UNKNOWN" | "CAT_REPOSITORY_UNKNOWN" => PackageError::agent_unknown(),
                _ => PackageError::library_unavailable(e.retryable, e.code),
            })?;
        if !view.offered || view.state_label == PartStateLabel::Withdrawn {
            return Err(PackageError::agent_not_offered("the agent is not offered"));
        }
        if view.state_label == PartStateLabel::NotForNewAgents {
            return Err(PackageError::deprecated_as_agent());
        }
        Ok(())
    }

    /// §J6: true when the installed agent may use the Mail Service's tools (`mail` server).
    pub async fn can_send_email(&self, agent_instance_id: AgentInstanceId) -> bool {
        let row = match self.live_row(agent_instance_id).await {
            Ok(row) => row,
            Err(_) => return false,
        };
        match row.may_do.as_deref().map(serde_json::from_str::<MayDo>) {
            Some(Ok(may_do)) => may_do.connections.iter().any(|c| c == "mail"),
            _ => {
                tracing::warn!(
                    component = "package_catalogue",
                    agent_instance_id = agent_instance_id.to_string().as_str(),
                    "agent.may_do_unreadable"
                );
                false
            }
        }
    }

    // spec: ordin-desktop/components/PackageCatalogue#SA-CLOSE-04
    /// §N: the pack questions an installed instance's plan left open, lowest `rule_index` first.
    pub async fn pending_choices(
        &self,
        instance: AgentInstanceId,
    ) -> Result<Vec<PendingChoice>, PackageError> {
        let row = self.live_row(instance).await?;
        if !matches!(
            row.status,
            AgentInstanceStatus::Ready | AgentInstanceStatus::Updating
        ) {
            return Err(PackageError::instance_not_ready(
                "the instance is not ready",
            ));
        }
        let stored: Vec<PendingPackChoice> = match serde_json::from_str(&row.pending_choices) {
            Ok(stored) => stored,
            Err(_) => {
                tracing::warn!(
                    component = "package_catalogue",
                    agent_instance_id = instance.to_string().as_str(),
                    "agent.pending_choices_unreadable"
                );
                return Err(PackageError::store(
                    false,
                    "the stored pack questions are not readable",
                ));
            }
        };
        let answered: Vec<u32> = {
            let mut conn = self.conn().await?;
            store::choices_of(&mut conn, instance)
                .await?
                .iter()
                .map(|c| c.rule_index)
                .collect()
        };
        let mut open: Vec<PendingPackChoice> = stored
            .into_iter()
            .filter(|p| !answered.contains(&p.rule_index))
            .collect();
        open.sort_by_key(|p| p.rule_index);
        let repository = row
            .agent_repository_id
            .filter(|_| !row.is_trial_of_own_draft);
        let mut out = Vec::with_capacity(open.len());
        for entry in open {
            let mut choices = Vec::with_capacity(entry.choices.len());
            for pack in &entry.choices {
                let label = match repository {
                    Some(repository) => self
                        .inner
                        .ports
                        .catalogue
                        .find_part(repository, PartKind::PracticePack, pack, None)
                        .await
                        .map(|view| view.part.name)
                        .unwrap_or_else(|_| pack.clone()),
                    None => pack.clone(),
                };
                choices.push(PendingChoiceOption {
                    pack: pack.clone(),
                    label,
                });
            }
            out.push(PendingChoice {
                agent_instance_id: instance,
                rule_index: entry.rule_index,
                at_least: entry.at_least,
                class_label: entry.class.unwrap_or_else(|| "pack".to_owned()),
                choices,
            });
        }
        Ok(out)
    }

    // spec: ordin-desktop/components/PackageCatalogue#SA-RECON-06
    /// §K: the non-removed instances serving one workspace.
    pub async fn agents_in_workspace(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<WorkspaceAgent>, PackageError> {
        Ok(self
            .workspace_rows(id)
            .await?
            .into_iter()
            .map(|r| WorkspaceAgent {
                agent_instance_id: r.id,
                display_name: r.display_name,
            })
            .collect())
    }
}
