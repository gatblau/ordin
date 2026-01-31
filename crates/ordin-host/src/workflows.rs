//! Workflows and their versions (sections E, F and G; steps 11, 12, 19–31): the check's facts, the
//! workflow commands, Ordin's drafts, import and export, repositories and sharing.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use ordin_mcp_front::{ToolFailure, ToolOutput};
use ordin_package_catalogue::AgentMatch;
use ordin_service_api::blocks::{Block, ResultBlock, ResultOffer};
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_service_api::commands::workflows::{
    AgentPresence, RepositoryFolder, RepositoryWorkflowFile, SubmissionId, VersionLine,
    WorkflowCheckReport, WorkflowSummary, WorkflowView,
};
use ordin_types::common::AgentRef;
use ordin_types::domain::{ShareHost, Workflow, WorkflowOrigin, WorkflowShare, WorkflowState};
use ordin_types::paging::{Cursor, Page};
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::{WorkflowFileAgent, WorkflowFileLimits, WorkflowFileStep};
use ordin_types::wire::WorkflowFile;
use ordin_types::{CrewRunId, RunId, WorkflowId, WorkspaceId};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::error::OhError;
use crate::host::{lock, OrdinHost, COMPONENT};
use crate::store::{self, WorkflowRow};
use crate::tools::fail;
use crate::types::*;
use crate::wff::{self, CheckFacts, CheckMode, Identity, Presence, Read};

fn count_steps(steps: &[WorkflowFileStep]) -> (usize, usize) {
    let mut agent = 0;
    let mut blocks = 0;
    for s in steps {
        match s {
            WorkflowFileStep::Agent { .. } => agent += 1,
            WorkflowFileStep::If { paths, .. } => {
                blocks += 1;
                for p in paths {
                    let (a, b) = count_steps(&p.steps);
                    agent += a;
                    blocks += b;
                }
            }
            WorkflowFileStep::Each { steps, .. } | WorkflowFileStep::Until { steps, .. } => {
                blocks += 1;
                let (a, b) = count_steps(steps);
                agent += a;
                blocks += b;
            }
            WorkflowFileStep::Call { .. } => blocks += 1,
        }
    }
    (agent, blocks)
}

fn host_name(host: ShareHost) -> &'static str {
    match host {
        ShareHost::Github => "github.com",
        ShareHost::Gitlab => "gitlab.com",
    }
}

fn origin_line(origin: &WorkflowOrigin, crew_date: Option<&Timestamp>) -> String {
    match (origin, crew_date) {
        (WorkflowOrigin::SavedFromCrewRun { .. }, Some(t)) => {
            let local: chrono::DateTime<chrono::Local> = t.0.into();
            format!("Saved from the run of {}", local.format("%-d %b"))
        }
        _ => "Drafted by Ordin".to_owned(),
    }
}

impl OrdinHost {
    // ---------------------------------------------------------------- section E (facts for the check)

    /// Gathers what step 19 needs, runs it, and says whether the workflow needs agents.
    pub(crate) async fn check_file(
        &self,
        ws: WorkspaceId,
        file: &mut WorkflowFile,
        mode: CheckMode,
        except: Option<WorkflowId>,
        unmatched: BTreeSet<usize>,
    ) -> Result<(WorkflowCheckReport, bool), OhError> {
        let policy = self.policy();
        let mut facts = CheckFacts {
            maxima: Some(policy.crew_maxima),
            profile_keys: policy.repository_profile_keys.clone(),
            unmatched,
            ..CheckFacts::default()
        };
        {
            let mut c = self.pool.acquire().await?;
            for row in store::workflows_of(&mut c, ws).await? {
                if Some(row.id) == except || row.state == WorkflowState::NotSavedYet {
                    continue;
                }
                if let Some(latest) = store::latest_version(&mut c, row.id).await? {
                    if let Some(v) = store::version(&mut c, row.id, latest).await? {
                        facts.workflows.insert(row.name.clone(), v.file);
                    }
                }
            }
        }
        for (i, a) in file.agents.iter().enumerate() {
            if facts.unmatched.contains(&i) {
                facts.presence.push(Presence::Missing);
                continue;
            }
            let presence = match self
                .ports
                .catalogue
                .match_workflow_agent(ws, &a.repository, &a.id, &a.version)
                .await
            {
                Ok(AgentMatch::Present {
                    agent_instance_id, ..
                }) => Presence::Present(agent_instance_id),
                Ok(AgentMatch::OtherVersion {
                    agent_instance_id,
                    installed_version,
                    ..
                }) => Presence::OtherVersion(agent_instance_id, installed_version),
                Ok(AgentMatch::Missing) | Err(_) => Presence::Missing,
            };
            if let Presence::Present(id) = presence {
                if self.ports.catalogue.can_send_email(id).await {
                    facts.sends_email.insert(id);
                }
            }
            facts.presence.push(presence);
        }
        let report = wff::check(file, &facts, mode);
        Ok((report, wff::needs_agents(&facts)))
    }

    async fn workflow_in(&self, workflow_id: WorkflowId) -> Result<WorkflowRow, OhError> {
        let mut c = self.pool.acquire().await?;
        store::workflow(&mut c, workflow_id)
            .await?
            .ok_or(OhError::WorkflowUnknown)
    }

    /// A name used in the workspace gets " (2)", " (3)" up to " (99)".
    async fn unique_name(&self, ws: WorkspaceId, name: &str) -> Result<(String, bool), OhError> {
        let mut c = self.pool.acquire().await?;
        if !store::name_taken(&mut c, ws, name, None).await? {
            return Ok((name.to_owned(), false));
        }
        for n in 2..=99 {
            let suffix = format!(" ({n})");
            let base: String = name.chars().take(80 - suffix.len()).collect();
            let candidate = format!("{base}{suffix}");
            if !store::name_taken(&mut c, ws, &candidate, None).await? {
                return Ok((candidate, true));
            }
        }
        Err(OhError::input("WFF_NAME_DUPLICATE", "/name"))
    }

    // ---------------------------------------------------------------- section F (commands)

    /// `workflows.list`.
    pub async fn workflows_list(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<Page<WorkflowSummary>, OhError> {
        let mut c = self.pool.acquire().await?;
        let mut all = Vec::new();
        for row in store::workflows_of(&mut c, workspace_id).await? {
            all.push(WorkflowSummary {
                workflow_id: row.id,
                name: row.name,
                state: row.state,
                latest_version: store::latest_version(&mut c, row.id).await?,
                shared: row.shared,
            });
        }
        let start: usize = match &cursor {
            Some(c) => c
                .decode()
                .map_err(|_| OhError::input("OH_INPUT_INVALID", "/cursor"))?,
            None => 0,
        };
        let end = (start + ordin_types::paging::PAGE_SIZE).min(all.len());
        Ok(Page {
            items: all
                .get(start..end)
                .map(<[WorkflowSummary]>::to_vec)
                .unwrap_or_default(),
            next_cursor: (end < all.len()).then(|| Cursor::encode(&end)),
        })
    }

    /// `workflows.get`: a saved version, or the draft of a workflow not saved yet.
    pub async fn workflows_get(
        &self,
        workflow_id: WorkflowId,
        version: Option<u32>,
    ) -> Result<WorkflowView, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let mut c = self.pool.acquire().await?;
        let versions = store::versions(&mut c, workflow_id).await?;
        let (shown, file) = match version {
            Some(v) => (
                Some(v),
                versions
                    .iter()
                    .find(|x| x.version == v)
                    .ok_or(OhError::VersionUnknown)?
                    .file
                    .clone(),
            ),
            None => match store::draft(&mut c, workflow_id).await? {
                Some(d) if versions.is_empty() || row.state == WorkflowState::NotSavedYet => {
                    (None, d.file)
                }
                _ => match versions.first() {
                    Some(v) => (Some(v.version), v.file.clone()),
                    None => return Err(OhError::VersionUnknown),
                },
            },
        };
        drop(c);
        let mut agents = Vec::new();
        for a in &file.agents {
            let presence = match self
                .ports
                .catalogue
                .match_workflow_agent(row.workspace_id, &a.repository, &a.id, &a.version)
                .await
            {
                Ok(AgentMatch::Present {
                    agent_instance_id, ..
                }) => AgentPresence::Present {
                    key: a.key.clone(),
                    agent: AgentRef::AgentInstance { agent_instance_id },
                },
                Ok(AgentMatch::OtherVersion {
                    agent_instance_id,
                    installed_version,
                    ..
                }) => AgentPresence::OtherVersion {
                    key: a.key.clone(),
                    agent: AgentRef::AgentInstance { agent_instance_id },
                    installed_version,
                },
                _ => AgentPresence::Missing { key: a.key.clone() },
            };
            agents.push(presence);
        }
        Ok(WorkflowView {
            workflow: Workflow {
                id: row.id,
                workspace_id: row.workspace_id,
                name: row.name,
                state: row.state,
                origin: row.origin,
                shared: row.shared,
                created_at: row.created_at,
            },
            version: shown,
            file,
            versions: versions
                .iter()
                .map(|v| VersionLine {
                    version: v.version,
                    change_note: v.change_note.clone(),
                    saved_at: v.saved_at,
                    restored_from_version: v.restored_from_version,
                })
                .collect(),
            agents,
        })
    }

    /// `workflows.check`: the check as **Save** would run it.
    pub async fn workflows_check(
        &self,
        workspace_id: WorkspaceId,
        mut file: WorkflowFile,
    ) -> Result<WorkflowCheckReport, OhError> {
        Ok(self
            .check_file(
                workspace_id,
                &mut file,
                CheckMode::Save,
                None,
                BTreeSet::new(),
            )
            .await?
            .0)
    }

    /// `workflows.save` (step 20).
    pub async fn workflows_save(
        &self,
        workflow_id: WorkflowId,
        base_version: Option<u32>,
        mut file: WorkflowFile,
        change_note: String,
    ) -> Result<u32, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let latest = {
            let mut c = self.pool.acquire().await?;
            store::latest_version(&mut c, workflow_id).await?
        };
        if let (Some(base), Some(latest)) = (base_version, latest) {
            if base != latest {
                return Err(OhError::StaleVersion { latest });
            }
        }
        let note = change_note.trim();
        if note.is_empty() || note.chars().count() > 200 || note.contains(['\n', '\r']) {
            return Err(OhError::input("WFF_CHANGE_NOTE_INVALID", "/change_note"));
        }
        {
            let mut c = self.pool.acquire().await?;
            if store::name_taken(&mut c, row.workspace_id, &file.name, Some(workflow_id)).await? {
                return Err(OhError::input("WFF_NAME_DUPLICATE", "/name"));
            }
        }
        // Step 26.4: the first save of an imported workflow runs the check in Import mode.
        let mode = if base_version.is_none() && row.state == WorkflowState::NotSavedYet {
            CheckMode::Import
        } else {
            CheckMode::Save
        };
        file.format = 3;
        let (report, needs) = self
            .check_file(
                row.workspace_id,
                &mut file,
                mode,
                Some(workflow_id),
                BTreeSet::new(),
            )
            .await?;
        if !report.problems.is_empty() {
            return Err(OhError::CheckFailed { report });
        }
        let version = latest.unwrap_or(0) + 1;
        let mut updated = row.clone();
        updated.name = file.name.clone();
        updated.state = if needs {
            WorkflowState::NeedsAgents
        } else {
            WorkflowState::Saved
        };
        if let Some(shared) = &mut updated.shared {
            shared.changed_here_not_sent = true;
        }
        let mut tx = self.pool.begin().await?;
        store::insert_version(
            &mut tx,
            workflow_id,
            version,
            note,
            &file,
            None,
            Timestamp::now(),
        )
        .await?;
        store::update_workflow(&mut tx, &updated).await?;
        store::delete_draft(&mut tx, workflow_id).await?;
        tx.commit().await?;
        let (steps, blocks) = count_steps(&file.steps);
        tracing::info!(
            component = COMPONENT,
            workflow_id = workflow_id.to_string().as_str(),
            version,
            steps,
            blocks,
            "ordin_host.workflow_saved"
        );
        self.workspace_changed(row.workspace_id).await;
        Ok(version)
    }

    /// `workflows.discard_draft` (step 27).
    pub async fn workflows_discard_draft(&self, workflow_id: WorkflowId) -> Result<(), OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let mut c = self.pool.acquire().await?;
        if row.state == WorkflowState::NotSavedYet {
            store::delete_workflow(&mut c, workflow_id).await?;
        } else {
            store::delete_draft(&mut c, workflow_id).await?;
        }
        drop(c);
        self.workspace_changed(row.workspace_id).await;
        Ok(())
    }

    /// `workflows.restore` (step 21).
    pub async fn workflows_restore(
        &self,
        workflow_id: WorkflowId,
        version: u32,
    ) -> Result<u32, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let mut tx = self.pool.begin().await?;
        let latest = store::latest_version(&mut tx, workflow_id)
            .await?
            .ok_or(OhError::VersionUnknown)?;
        let old = store::version(&mut tx, workflow_id, version)
            .await?
            .ok_or(OhError::VersionUnknown)?;
        if version == latest {
            return Err(OhError::input("OH_RESTORE_SAME", "/version"));
        }
        let next = latest + 1;
        store::insert_version(
            &mut tx,
            workflow_id,
            next,
            &format!("Restored version {version}"),
            &old.file,
            Some(version),
            Timestamp::now(),
        )
        .await?;
        tx.commit().await?;
        self.workspace_changed(row.workspace_id).await;
        Ok(next)
    }

    /// `workflows.rename` (step 22): no version is written.
    pub async fn workflows_rename(
        &self,
        workflow_id: WorkflowId,
        name: String,
    ) -> Result<(), OhError> {
        let mut row = self.workflow_in(workflow_id).await?;
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(OhError::input("WFF_NAME_INVALID", "/name"));
        }
        let mut c = self.pool.acquire().await?;
        if store::name_taken(&mut c, row.workspace_id, &name, Some(workflow_id)).await? {
            return Err(OhError::input("WFF_NAME_DUPLICATE", "/name"));
        }
        row.name = name;
        store::update_workflow(&mut c, &row).await?;
        drop(c);
        self.workspace_changed(row.workspace_id).await;
        Ok(())
    }

    /// `workflows.duplicate` (step 23).
    pub async fn workflows_duplicate(
        &self,
        workflow_id: WorkflowId,
        version: u32,
        name: String,
        to_workspace_id: WorkspaceId,
    ) -> Result<WorkflowId, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        self.ports
            .workspaces
            .ensure_active(to_workspace_id)
            .await
            .map_err(|_| OhError::WorkspaceUnknown)?;
        let mut file = {
            let mut c = self.pool.acquire().await?;
            store::version(&mut c, workflow_id, version)
                .await?
                .ok_or(OhError::VersionUnknown)?
                .file
        };
        let wanted = if name.trim().is_empty() {
            format!("Copy of {}", row.name)
        } else {
            name.trim().to_owned()
        };
        let wanted: String = wanted.chars().take(80).collect();
        let (name, _) = self.unique_name(to_workspace_id, &wanted).await?;
        file.name = name.clone();
        let (report, needs) = self
            .check_file(
                to_workspace_id,
                &mut file,
                CheckMode::Duplicate,
                None,
                BTreeSet::new(),
            )
            .await?;
        if !report.problems.is_empty() {
            return Err(OhError::CheckFailed { report });
        }
        let new = WorkflowRow {
            id: WorkflowId::new(),
            workspace_id: to_workspace_id,
            name,
            state: if needs {
                WorkflowState::NeedsAgents
            } else {
                WorkflowState::Saved
            },
            origin: WorkflowOrigin::Duplicated {
                from_workflow_id: workflow_id,
                from_version: version,
            },
            shared: None,
            created_at: Timestamp::now(),
        };
        let note: String = format!("Copied from {} version {version}", row.name)
            .chars()
            .take(200)
            .collect();
        let mut tx = self.pool.begin().await?;
        store::insert_workflow(&mut tx, &new).await?;
        store::insert_version(&mut tx, new.id, 1, &note, &file, None, Timestamp::now()).await?;
        tx.commit().await?;
        self.workspace_changed(to_workspace_id).await;
        Ok(new.id)
    }

    /// `workflows.remove` (step 24): the screen's confirmation is enough (SA-RECON-09).
    // spec: ordin-desktop/components/OrdinHost#SA-RECON-09
    pub async fn workflows_remove(
        &self,
        workflow_id: WorkflowId,
        confirmed: bool,
    ) -> Result<(), OhError> {
        if !confirmed {
            return Err(OhError::NotConfirmed);
        }
        let row = self.workflow_in(workflow_id).await?;
        let mut c = self.pool.acquire().await?;
        store::delete_workflow(&mut c, workflow_id).await?;
        drop(c);
        self.workspace_changed(row.workspace_id).await;
        Ok(())
    }

    /// `workflows.export` (step 25): format 3 YAML text; the screens write it where the person chooses.
    // spec: ordin-desktop/components/OrdinHost#SA-RECON-05
    pub async fn workflows_export(
        &self,
        workflow_id: WorkflowId,
        version: u32,
    ) -> Result<(String, String), OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let mut file = {
            let mut c = self.pool.acquire().await?;
            store::version(&mut c, workflow_id, version)
                .await?
                .ok_or(OhError::VersionUnknown)?
                .file
        };
        file.format = 3;
        file.name = row.name.clone();
        let yaml = wff::to_yaml(&file);
        if yaml.len() > wff::FILE_MAX_BYTES {
            return Err(OhError::FileRefused {
                code: "WFF_FILE_TOO_LARGE",
            });
        }
        tracing::info!(
            component = COMPONENT,
            workflow_id = workflow_id.to_string().as_str(),
            version,
            bytes = yaml.len(),
            "ordin_host.workflow_exported"
        );
        Ok((format!("{}.workflow.yaml", row.name), yaml))
    }

    /// Steps 26.2–26.3: the text into a checked format-3 file.
    async fn read_into(
        &self,
        ws: WorkspaceId,
        yaml: &str,
        mode: CheckMode,
    ) -> Result<(WorkflowFile, WorkflowCheckReport, bool, u8), OhError> {
        let read = wff::read(yaml).map_err(|r| OhError::FileRefused { code: r.0 })?;
        let (mut file, mut legacy_problems, unmatched, format) = match read {
            Read::Current(f) => (f, Vec::new(), BTreeSet::new(), 3),
            Read::Legacy {
                format,
                value,
                agents,
            } => {
                let mut identities = Vec::new();
                for a in &agents {
                    let id = match self.ports.catalogue.match_by_name(ws, &a.name).await {
                        Ok(AgentMatch::Present {
                            agent_instance_id, ..
                        })
                        | Ok(AgentMatch::OtherVersion {
                            agent_instance_id, ..
                        }) => self
                            .ports
                            .catalogue
                            .agent_identity(agent_instance_id)
                            .await
                            .ok()
                            .map(|(repository, id, _, name)| Identity {
                                repository,
                                id,
                                name,
                            }),
                        _ => None,
                    };
                    identities.push(id);
                }
                let (file, problems, unmatched) =
                    wff::build_legacy(format, &value, &agents, &identities)
                        .map_err(|r| OhError::FileRefused { code: r.0 })?;
                (file, problems, unmatched, format)
            }
        };
        let (mut report, needs) = self
            .check_file(ws, &mut file, mode, None, unmatched)
            .await?;
        legacy_problems.append(&mut report.problems);
        report.problems = legacy_problems;
        Ok((file, report, needs, format))
    }

    /// Inserts an imported or repository workflow, not saved yet, with its draft (step 26.4).
    async fn add_draft(
        &self,
        ws: WorkspaceId,
        mut file: WorkflowFile,
        mut report: WorkflowCheckReport,
        origin: WorkflowOrigin,
        shared: Option<WorkflowShare>,
        drafted_by: &str,
    ) -> Result<(WorkflowId, WorkflowCheckReport), OhError> {
        let wanted = file.name.clone();
        let (name, suffixed) = self.unique_name(ws, &wanted).await?;
        if suffixed {
            report.notices.push(wff::problem(
                "OH_NAME_SUFFIXED",
                "/name",
                &[("name", wanted.clone()), ("new_name", name.clone())],
            ));
        }
        file.name = name.clone();
        let row = WorkflowRow {
            id: WorkflowId::new(),
            workspace_id: ws,
            name,
            state: WorkflowState::NotSavedYet,
            origin,
            shared,
            created_at: Timestamp::now(),
        };
        let mut tx = self.pool.begin().await?;
        store::insert_workflow(&mut tx, &row).await?;
        store::set_draft(
            &mut tx,
            row.id,
            None,
            &file,
            &report,
            drafted_by,
            Timestamp::now(),
        )
        .await?;
        tx.commit().await?;
        self.workspace_changed(ws).await;
        Ok((row.id, report))
    }

    /// `workflows.import` (step 26). Nothing is read by Ordin's model; the file grants nothing.
    pub async fn workflows_import(
        &self,
        workspace_id: WorkspaceId,
        file_name: String,
        yaml: String,
    ) -> Result<(WorkflowId, WorkflowCheckReport), OhError> {
        if yaml.len() > wff::FILE_MAX_BYTES {
            return Err(OhError::FileRefused {
                code: "WFF_FILE_TOO_LARGE",
            });
        }
        let n = file_name.chars().count();
        if n == 0 || n > 255 || file_name.contains(['/', '\\']) {
            return Err(OhError::input("OH_INPUT_INVALID", "/file_name"));
        }
        self.ports
            .workspaces
            .ensure_active(workspace_id)
            .await
            .map_err(|_| OhError::WorkspaceUnknown)?;
        let (file, report, _, format) = self
            .read_into(workspace_id, &yaml, CheckMode::Import)
            .await?;
        let origin = WorkflowOrigin::Imported {
            file_name,
            imported_on: Timestamp::now(),
        };
        let (id, report) = self
            .add_draft(workspace_id, file, report, origin, None, "import")
            .await?;
        tracing::info!(
            component = COMPONENT,
            workspace_id = workspace_id.to_string().as_str(),
            workflow_id = id.to_string().as_str(),
            format,
            problems = report.problems.len(),
            notices = report.notices.len(),
            "ordin_host.workflow_imported"
        );
        Ok((id, report))
    }

    // ---------------------------------------------------------------- section G (repositories)

    /// The folders of step 28: the workspace's linked repositories (their root) and the company's.
    async fn folders(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<(RepositoryFolder, String, bool)>, OhError> {
        let mut out = Vec::new();
        let linked = self
            .ports
            .workspaces
            .linked_repositories(ws)
            .await
            .map_err(|e| self.dep("workspaces", e))?;
        for r in &linked {
            out.push((
                RepositoryFolder {
                    host: r.host,
                    repository: r.repository.clone(),
                    folder: String::new(),
                },
                r.host_name.clone(),
                false,
            ));
        }
        if let Some(company) = self.policy().company_workflow_repository {
            // OrdinHost.md Internal Logic step 28: the host name is the public one of the `ShareHost`
            // unless the workspace links the same repository.
            let name = linked
                .iter()
                .find(|r| r.repository == company.repository)
                .map(|r| r.host_name.clone())
                .unwrap_or_else(|| host_name(company.host).to_owned());
            out.push((company, name, true));
        }
        Ok(out)
    }

    /// `workflows.list_in_repositories` (step 28); a folder that cannot be read is skipped.
    pub async fn workflows_list_in_repositories(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<RepositoryWorkflowFile>, OhError> {
        let mut out = Vec::new();
        for (folder, name, company) in self.folders(workspace_id).await? {
            let r = ordin_projects::RepoFolderRef {
                host: name,
                repository: folder.repository.clone(),
                folder: folder.folder.clone(),
            };
            match self.ports.projects.list_files(&r, "*.workflow.yaml").await {
                Ok(files) => {
                    for f in files {
                        out.push(RepositoryWorkflowFile {
                            folder: folder.clone(),
                            file_name: f.name,
                            company,
                            last_changed_by: f.last_changed_by,
                            last_changed_at: f.last_changed_at,
                        });
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        component = COMPONENT,
                        host = r.host.as_str(),
                        repository = r.repository.as_str(),
                        code = e.code.as_str(),
                        "ordin_host.repository_unreachable"
                    );
                }
            }
        }
        Ok(out)
    }

    async fn host_for(
        &self,
        ws: WorkspaceId,
        folder: &RepositoryFolder,
    ) -> Result<String, OhError> {
        Ok(self
            .folders(ws)
            .await?
            .into_iter()
            .find(|(f, _, _)| f.repository == folder.repository && f.host == folder.host)
            .map(|(_, name, _)| name)
            .unwrap_or_else(|| host_name(folder.host).to_owned()))
    }

    /// `workflows.add_from_repository` (step 29).
    pub async fn workflows_add_from_repository(
        &self,
        workspace_id: WorkspaceId,
        host: ShareHost,
        repository: String,
        folder: String,
        file_name: String,
    ) -> Result<(WorkflowId, WorkflowCheckReport), OhError> {
        let rf = RepositoryFolder {
            host,
            repository: repository.clone(),
            folder: folder.clone(),
        };
        let r = ordin_projects::RepoFolderRef {
            host: self.host_for(workspace_id, &rf).await?,
            repository: repository.clone(),
            folder: folder.clone(),
        };
        let (bytes, commit) = self
            .ports
            .projects
            .read_file(&r, &file_name)
            .await
            .map_err(|e| self.dep("projects", e))?;
        let text = String::from_utf8(bytes).map_err(|_| OhError::FileRefused {
            code: "WFF_FILE_MALFORMED",
        })?;
        let (file, report, _, _) = self
            .read_into(workspace_id, &text, CheckMode::RepositoryRead)
            .await?;
        let changed_by = self
            .ports
            .projects
            .list_files(&r, "*.workflow.yaml")
            .await
            .ok()
            .and_then(|files| files.into_iter().find(|f| f.name == file_name))
            .map(|f| f.last_changed_by)
            .unwrap_or_default();
        let shared = WorkflowShare {
            host,
            repository,
            folder,
            file_name: file_name.clone(),
            followed_date: Timestamp::now(),
            followed_by: changed_by,
            followed_commit: commit,
            changed_here_not_sent: false,
            newer_in_repository: false,
        };
        let origin = WorkflowOrigin::Imported {
            file_name,
            imported_on: Timestamp::now(),
        };
        self.add_draft(
            workspace_id,
            file,
            report,
            origin,
            Some(shared),
            "repository",
        )
        .await
    }

    /// `workflows.share` (step 30): through the Push Service, which opens the approval card.
    pub async fn workflows_share(
        &self,
        workflow_id: WorkflowId,
        version: u32,
        folder: RepositoryFolder,
    ) -> Result<SubmissionId, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let ws = row.workspace_id;
        let linked = self
            .ports
            .workspaces
            .linked_repositories(ws)
            .await
            .map_err(|e| self.dep("workspaces", e))?;
        let Some(repo) = linked
            .iter()
            .find(|r| r.repository == folder.repository && r.host == folder.host)
        else {
            return Err(OhError::FolderNotLinked);
        };
        let (file_name, yaml) = self.workflows_export(workflow_id, version).await?;
        let notes: Vec<String> = {
            let mut c = self.pool.acquire().await?;
            store::versions(&mut c, workflow_id)
                .await?
                .into_iter()
                .map(|v| v.change_note)
                .collect()
        };
        let base = self
            .ports
            .projects
            .default_branch(&repo.host_name, &repo.repository)
            .await
            .map_err(|e| self.dep("projects", e))?;
        let path = if folder.folder.is_empty() {
            file_name.clone()
        } else {
            format!("{}/{file_name}", folder.folder)
        };
        // OrdinHost.md Internal Logic step 30: the card says what is shared and where.
        let card = ordin_push::CardText {
            question: format!("Share the workflow {}?", row.name),
            verb: "Share".to_owned(),
            where_line: format!(
                "{} › {}",
                repo.repository,
                if folder.folder.is_empty() {
                    "/"
                } else {
                    &folder.folder
                }
            ),
            affects: vec![],
            changes: notes.join("\n"),
            content: None,
        };
        let submission = self
            .ports
            .push
            .propose_pull_request(ordin_push::PullRequestProposal {
                workspace_id: ws,
                origin: ordin_push::SubmissionOrigin::WorkflowShare {
                    workspace_id: ws,
                    file_name: file_name.clone(),
                },
                host: repo.host_name.clone(),
                repository: repo.repository.clone(),
                content: ordin_push::PullRequestContent::Files {
                    files: vec![(path, yaml.into_bytes())],
                },
                branch: format!("ordin/workflow-{workflow_id}-v{version}"),
                base,
                title: format!("Share workflow {}", row.name),
                body: notes.join("\n"),
                labels: vec![],
                reviewers: vec![],
                existing: None,
                card,
            })
            .await
            .map_err(|e| self.dep("push", e))?;
        let text = submission.0.hyphenated().to_string();
        lock(&self.state).shares.insert(text.clone(), workflow_id);
        let mut updated = row.clone();
        if updated.shared.is_none() {
            updated.shared = Some(WorkflowShare {
                host: folder.host,
                repository: folder.repository.clone(),
                folder: folder.folder.clone(),
                file_name,
                followed_date: Timestamp::now(),
                followed_by: String::new(),
                followed_commit: String::new(),
                changed_here_not_sent: true,
                newer_in_repository: false,
            });
            let mut c = self.pool.acquire().await?;
            store::update_workflow(&mut c, &updated).await?;
        }
        SubmissionId::parse(&text).map_err(|_| OhError::DatastoreUnavailable)
    }

    /// `workflows.send_back`: share the latest version to the shared folder.
    pub async fn workflows_send_back(
        &self,
        workflow_id: WorkflowId,
    ) -> Result<SubmissionId, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let shared = row.shared.ok_or(OhError::FolderNotLinked)?;
        let latest = {
            let mut c = self.pool.acquire().await?;
            store::latest_version(&mut c, workflow_id)
                .await?
                .ok_or(OhError::VersionUnknown)?
        };
        self.workflows_share(
            workflow_id,
            latest,
            RepositoryFolder {
                host: shared.host,
                repository: shared.repository,
                folder: shared.folder,
            },
        )
        .await
    }

    /// `workflows.use_newer` (step 31): the repository's file as a new local version; nothing merged.
    pub async fn workflows_use_newer(&self, workflow_id: WorkflowId) -> Result<u32, OhError> {
        let row = self.workflow_in(workflow_id).await?;
        let shared = row.shared.clone().ok_or(OhError::FolderNotLinked)?;
        let rf = RepositoryFolder {
            host: shared.host,
            repository: shared.repository.clone(),
            folder: shared.folder.clone(),
        };
        let r = ordin_projects::RepoFolderRef {
            host: self.host_for(row.workspace_id, &rf).await?,
            repository: shared.repository.clone(),
            folder: shared.folder.clone(),
        };
        let (bytes, commit) = self
            .ports
            .projects
            .read_file(&r, &shared.file_name)
            .await
            .map_err(|e| self.dep("projects", e))?;
        let text = String::from_utf8(bytes).map_err(|_| OhError::FileRefused {
            code: "WFF_FILE_MALFORMED",
        })?;
        let (mut file, report, needs, _) = self
            .read_into(row.workspace_id, &text, CheckMode::RepositoryRead)
            .await?;
        if !report.problems.is_empty() {
            return Err(OhError::CheckFailed { report });
        }
        file.name = row.name.clone();
        let mut tx = self.pool.begin().await?;
        let version = store::latest_version(&mut tx, workflow_id)
            .await?
            .unwrap_or(0)
            + 1;
        let note = format!("Newer version from {}", r.host);
        store::insert_version(
            &mut tx,
            workflow_id,
            version,
            &note,
            &file,
            None,
            Timestamp::now(),
        )
        .await?;
        let mut updated = row.clone();
        updated.state = if needs {
            WorkflowState::NeedsAgents
        } else {
            WorkflowState::Saved
        };
        if let Some(s) = &mut updated.shared {
            s.followed_commit = commit;
            s.followed_date = Timestamp::now();
            s.newer_in_repository = false;
        }
        store::update_workflow(&mut tx, &updated).await?;
        tx.commit().await?;
        self.workspace_changed(row.workspace_id).await;
        Ok(version)
    }

    /// `workflows.keep_as_own`: clears `shared`.
    pub async fn workflows_keep_as_own(&self, workflow_id: WorkflowId) -> Result<(), OhError> {
        let mut row = self.workflow_in(workflow_id).await?;
        row.shared = None;
        let mut c = self.pool.acquire().await?;
        store::update_workflow(&mut c, &row).await?;
        Ok(())
    }

    /// Step 31: each shared workflow's file in its repository; a commit other than the followed one
    /// is noticed, never used.
    pub(crate) async fn recheck(&self) {
        let rows = match self.pool.acquire().await {
            Ok(mut c) => store::shared_workflows(&mut c).await.unwrap_or_default(),
            Err(_) => return,
        };
        for row in rows {
            let Some(shared) = row.shared.clone() else {
                continue;
            };
            let rf = RepositoryFolder {
                host: shared.host,
                repository: shared.repository.clone(),
                folder: shared.folder.clone(),
            };
            let Ok(host) = self.host_for(row.workspace_id, &rf).await else {
                continue;
            };
            let r = ordin_projects::RepoFolderRef {
                host: host.clone(),
                repository: shared.repository.clone(),
                folder: shared.folder.clone(),
            };
            let commit = match self
                .ports
                .projects
                .file_last_commit(&r, &shared.file_name)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        component = COMPONENT,
                        host = host.as_str(),
                        repository = shared.repository.as_str(),
                        code = e.code.as_str(),
                        "ordin_host.repository_unreachable"
                    );
                    continue;
                }
            };
            if commit == shared.followed_commit {
                continue;
            }
            let mut updated = row.clone();
            let Some(s) = updated.shared.as_mut() else {
                continue;
            };
            if s.followed_commit.is_empty() && !s.changed_here_not_sent {
                // The merged share: the file now in the repository is the one sent (step 30).
                s.followed_commit = commit;
                s.followed_date = Timestamp::now();
            } else {
                s.newer_in_repository = true;
            }
            if let Ok(mut c) = self.pool.acquire().await {
                let _ = store::update_workflow(&mut c, &updated).await;
            }
        }
    }

    // ---------------------------------------------------------------- Ordin's drafts (steps 11, 12)

    async fn draft_agents(
        &self,
        ws: WorkspaceId,
        agents: &[DraftAgentArg],
    ) -> Result<Vec<WorkflowFileAgent>, ToolFailure> {
        let mut out = Vec::new();
        for a in agents {
            let Some(hit) = self.directory_hit(ws, &a.agent).await? else {
                // A remote agent is not in the directory of installed agents (assumed in step 11).
                return Err(fail(
                    "OH_REMOTE_AGENT_IN_WORKFLOW",
                    "Workflows name agents from the library only.",
                    false,
                ));
            };
            let (repository, id, version, name) = self
                .ports
                .catalogue
                .agent_identity(hit.agent_instance_id)
                .await
                .map_err(|e| crate::tools::dep_fail(&e))?;
            out.push(WorkflowFileAgent {
                key: a.key.clone(),
                repository,
                id,
                version,
                name,
                role: a.role.clone(),
            });
        }
        Ok(out)
    }

    fn default_limits(&self) -> WorkflowFileLimits {
        let d = self.policy().crew_defaults;
        WorkflowFileLimits {
            max_rounds: d.rounds,
            wall_clock_minutes: d.wall_clock_minutes,
            request_budget: d.request_budget,
            agents_at_once: d.members_at_once,
            failure_repeat_limit: d.failure_repeat_limit,
            external_action_agents: vec![],
        }
    }

    async fn offer_draft(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        mut file: WorkflowFile,
        origin: WorkflowOrigin,
    ) -> Result<ToolOutput, ToolFailure> {
        let failed = |e: OhError| fail(&e.code(), "That did not work; try again.", e.retryable());
        let (report, _) = self
            .check_file(ws, &mut file, CheckMode::Draft, None, BTreeSet::new())
            .await
            .map_err(failed)?;
        let (id, report) = self
            .add_draft(ws, file, report, origin, None, "ordin")
            .await
            .map_err(failed)?;
        self.show_block(
            ws,
            MessageAuthor::Ordin,
            Block::Result(ResultBlock::Offer {
                offer: ResultOffer::SaveAsWorkflow,
                run_id: Some(run_id),
            }),
            true,
        )
        .await
        .map_err(failed)?;
        let mut text = "Draft shown; the person saves it.".to_owned();
        for p in &report.problems {
            text.push_str(&format!("\nProblem {} at {}", p.code, p.at));
        }
        Ok(ToolOutput {
            text,
            structured: Some(json!({ "workflow_id": id })),
        })
    }

    /// Step 11 (`save_workflow`).
    pub(crate) async fn save_workflow_draft(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        a: SaveWorkflowArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let agents = self.draft_agents(ws, &a.agents).await?;
        let (origin, date) = match a.crew_run_id {
            Some(id) => (
                WorkflowOrigin::SavedFromCrewRun { crew_run_id: id },
                Some(Timestamp::now()),
            ),
            None => (WorkflowOrigin::DraftedByOrdin, None),
        };
        let file = WorkflowFile {
            format: 3,
            name: a.name,
            purpose: a.purpose,
            inputs: a.inputs,
            agents,
            steps: a.steps,
            limits: a.limits.unwrap_or_else(|| self.default_limits()),
            origin: origin_line(&origin, date.as_ref()),
        };
        self.offer_draft(ws, run_id, file, origin).await
    }

    /// `keep_crew`: a workflow drafted from the crew run that just finished.
    pub(crate) async fn keep_crew(
        &self,
        ws: WorkspaceId,
        run_id: RunId,
        a: KeepCrewArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let row = {
            let mut c = self.pool.acquire().await.map_err(|_| {
                fail(
                    "OH_DATASTORE_UNAVAILABLE",
                    "Something went wrong; try again.",
                    true,
                )
            })?;
            store::charter_by_run(&mut c, a.crew_run_id)
                .await
                .ok()
                .flatten()
        }
        .filter(|r| r.charter.workspace_id == ws)
        .ok_or_else(|| {
            fail(
                "OH_CHARTER_UNKNOWN",
                "No crew run with that id is in this workspace.",
                false,
            )
        })?;
        // OrdinHost.md Internal Logic step 11: the steps a coordinated crew took are not readable, so
        // the draft holds one step per member, in the charter's order, for the person to edit.
        let mut agents = Vec::new();
        let mut steps = Vec::new();
        let mut keys = BTreeSet::new();
        for (i, m) in row.charter.members.iter().enumerate() {
            let AgentRef::AgentInstance { agent_instance_id } = m.member else {
                return Err(fail(
                    "OH_REMOTE_AGENT_IN_WORKFLOW",
                    "Workflows name agents from the library only.",
                    false,
                ));
            };
            let (repository, id, version, name) = self
                .ports
                .catalogue
                .agent_identity(agent_instance_id)
                .await
                .map_err(|e| crate::tools::dep_fail(&e))?;
            let key = wff::legacy_key(&name, &keys);
            keys.insert(key.clone());
            steps.push(WorkflowFileStep::Agent {
                position: (i + 1).to_string(),
                agent: key.clone(),
                line: m.role.chars().take(200).collect(),
                uses: vec![],
                asks_first: false,
                ordin_checks: false,
                if_it_fails: Default::default(),
            });
            agents.push(WorkflowFileAgent {
                key,
                repository,
                id,
                version,
                name,
                role: m.role.chars().take(120).collect(),
            });
        }
        let origin = WorkflowOrigin::SavedFromCrewRun {
            crew_run_id: a.crew_run_id,
        };
        let file = WorkflowFile {
            format: 3,
            name: a.name,
            purpose: row.charter.purpose.chars().take(200).collect(),
            inputs: vec![],
            agents,
            steps,
            limits: self.default_limits(),
            origin: origin_line(&origin, Some(&row.charter.written_at)),
        };
        self.offer_draft(ws, run_id, file, origin).await
    }

    /// Step 12 (`propose_workflow_change`).
    pub(crate) async fn propose_workflow_change(
        &self,
        ws: WorkspaceId,
        a: ProposeWorkflowChangeArgs,
    ) -> Result<ToolOutput, ToolFailure> {
        let failed = |e: OhError| fail(&e.code(), "That did not work; try again.", e.retryable());
        let row = self.workflow_in(a.workflow_id).await.map_err(failed)?;
        if row.workspace_id != ws {
            return Err(fail(
                "OH_WORKFLOW_UNKNOWN",
                "There's no saved workflow with that id in this workspace.",
                false,
            ));
        }
        let (latest, mut file) = {
            let mut c = self.pool.acquire().await.map_err(|e| failed(e.into()))?;
            let latest = store::latest_version(&mut c, a.workflow_id)
                .await
                .map_err(|e| failed(e.into()))?;
            let Some(latest) = latest else {
                return Err(fail(
                    "OH_WORKFLOW_UNKNOWN",
                    "There's no saved workflow with that id in this workspace.",
                    false,
                ));
            };
            let v = store::version(&mut c, a.workflow_id, latest)
                .await
                .map_err(|e| failed(e.into()))?;
            (
                latest,
                v.map(|v| v.file)
                    .ok_or_else(|| failed(OhError::VersionUnknown))?,
            )
        };
        if a.base_version != latest {
            return Err(fail(
                "OH_STALE_VERSION",
                &format!("The latest version is {latest}."),
                false,
            ));
        }
        file.steps = a.steps;
        if let Some(p) = a.purpose {
            file.purpose = p;
        }
        if let Some(i) = a.inputs {
            file.inputs = i;
        }
        if let Some(l) = a.limits {
            file.limits = l;
        }
        let (report, _) = self
            .check_file(
                ws,
                &mut file,
                CheckMode::Draft,
                Some(a.workflow_id),
                BTreeSet::new(),
            )
            .await
            .map_err(failed)?;
        {
            let mut c = self.pool.acquire().await.map_err(|e| failed(e.into()))?;
            store::set_draft(
                &mut c,
                a.workflow_id,
                Some(latest),
                &file,
                &report,
                "ordin",
                Timestamp::now(),
            )
            .await
            .map_err(|e| failed(e.into()))?;
        }
        self.show_block(
            ws,
            MessageAuthor::Ordin,
            Block::Result(ResultBlock::Offer {
                offer: ResultOffer::UpdateTheWorkflow,
                run_id: None,
            }),
            true,
        )
        .await
        .map_err(failed)?;
        let _ = a.change_note;
        Ok(ToolOutput {
            text: "Change shown; the person saves it.".into(),
            structured: None,
        })
    }
}

/// The repository re-check task (step 31), every `repository_check_period`.
pub(crate) fn spawn_recheck(host: &Arc<OrdinHost>, stop: CancellationToken) {
    let me = Arc::downgrade(host);
    let period = host.config.repository_check_period;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(period) => {}
            }
            let Some(host) = me.upgrade() else { return };
            host.recheck().await;
        }
    });
}

/// Share outcomes from the Push Service (step 30).
pub(crate) fn spawn_share_listener(host: &Arc<OrdinHost>, stop: CancellationToken) {
    let me = Arc::downgrade(host);
    let mut notices = host.ports.push.subscribe();
    tokio::spawn(async move {
        loop {
            let notice = tokio::select! {
                _ = stop.cancelled() => return,
                n = notices.recv() => n,
            };
            let notice = match notice {
                Ok(n) => n,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            };
            let Some(host) = me.upgrade() else { return };
            if let ordin_push::PushNotice::PushFinished {
                pending_push_id,
                workspace_id,
                failed,
                ..
            } = &notice
            {
                host.send_back_failed(*workspace_id, *pending_push_id, failed)
                    .await;
                continue;
            }
            let (submission, outcome) = match &notice {
                ordin_push::PushNotice::PullRequestOpened { submission, .. } => {
                    (submission, "opened")
                }
                ordin_push::PushNotice::PullRequestFailed { submission, .. } => {
                    (submission, "failed")
                }
                ordin_push::PushNotice::PullRequestRejected { submission, .. } => {
                    (submission, "rejected")
                }
                _ => continue,
            };
            let key = submission.0.hyphenated().to_string();
            let Some(workflow_id) = lock(&host.state).shares.get(&key).copied() else {
                continue;
            };
            tracing::info!(
                component = COMPONENT,
                workflow_id = workflow_id.to_string().as_str(),
                pending_push_id = key.to_string().as_str(),
                outcome,
                "ordin_host.share_outcome"
            );
            if outcome == "opened" {
                if let Ok(mut c) = host.pool.acquire().await {
                    if let Ok(Some(mut row)) = store::workflow(&mut c, workflow_id).await {
                        if let Some(s) = &mut row.shared {
                            s.changed_here_not_sent = false;
                        }
                        let _ = store::update_workflow(&mut c, &row).await;
                    }
                }
            }
        }
    });
}

/// Keeps the crew-run identifier type referenced by `keep_crew` in this module's imports.
#[allow(dead_code)]
type KeptRun = (CrewRunId, HashMap<String, String>);
