//! Building the requests every flow sends (PackageCatalogue.md §A0): the Resolver's request, the
//! mapping of its errors, "Uses" and the Package Inspector's request.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ordin_catalogue::PartStateLabel;
use ordin_installers::HarnessLaunch;
use ordin_package_inspector::{InspectRequest, InstallFacts, PartState};
use ordin_resolver::model::{
    ChosenBy, RepositoryRef, ResolutionPlan, SourceScope, BUNDLED_REPOSITORY_ID,
};
use ordin_resolver::{PackAnswer, ResolveError, ResolveRequest};
use ordin_types::domain::HarnessKind;
use ordin_types::{AgentInstanceId, AgentRepositoryId};

use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::store::{ChoiceSource, StoredChoice};
use crate::types::UsesItem;

/// What a re-plan needs besides the agent: the languages and locale the instance was added with,
/// its harness and its stored choices.
#[derive(Debug, Clone)]
pub(crate) struct PlanInputs {
    pub languages: Vec<String>,
    pub locale: String,
    pub harness: HarnessKind,
    pub choices: Vec<StoredChoice>,
}

/// §A0.2.
pub(crate) fn resolve_request(
    scope: SourceScope,
    identifier: &str,
    version: Option<String>,
    inputs: &PlanInputs,
) -> ResolveRequest {
    let pack_answers = inputs
        .choices
        .iter()
        .filter(|c| c.source == ChoiceSource::AgentAsked)
        .map(|c| PackAnswer {
            rule_index: c.rule_index,
            pack_ids: c.packs.clone(),
        })
        .collect();
    let pack_changes: BTreeSet<String> = inputs
        .choices
        .iter()
        .filter(|c| c.source == ChoiceSource::PersonChange)
        .flat_map(|c| c.packs.iter().cloned())
        .collect();
    ResolveRequest {
        scope,
        root_id: identifier.to_owned(),
        root_version: version,
        target_harness: inputs.harness,
        workspace_languages: inputs.languages.clone(),
        interface_language: inputs.locale.clone(),
        pack_answers,
        pack_changes: pack_changes.into_iter().collect(),
        overlay_changes: Vec::new(),
        replacements: Vec::new(),
        staged: Vec::new(),
        allow_draft_root: false,
        source: None,
    }
}

/// §A0.3: what a `ResolveError` means for an add.
#[derive(Debug)]
pub(crate) enum Failure {
    PackMissing(String),
    /// A permanent resolution error: the agent cannot be added safely; the code is for the journal.
    Unsafe(&'static str),
    Err(PackageError),
}

pub(crate) fn map_resolve_error(error: &ResolveError) -> Failure {
    match error {
        ResolveError::RequiredPackUnsettled { class, .. } => {
            Failure::PackMissing(class.clone().unwrap_or_else(|| "pack".to_owned()))
        }
        ResolveError::PartWithdrawn { .. } => Failure::Err(PackageError::agent_not_offered(
            "a part of the agent is withdrawn",
        )),
        other if other.retryable() => Failure::Err(PackageError::check_unavailable(other.code())),
        other => Failure::Unsafe(other.code()),
    }
}

/// `RepositoryRef` of the row's repository: none or the bundled id means the bundled catalogue.
pub(crate) fn home_of(repository: Option<AgentRepositoryId>) -> RepositoryRef {
    match repository {
        Some(id) if id != BUNDLED_REPOSITORY_ID => RepositoryRef::Repository(id),
        _ => RepositoryRef::Bundled,
    }
}

/// The repositories by the label a `PartRef` carries (`<host>/<repository>` or `bundled`).
#[derive(Debug, Default, Clone)]
pub(crate) struct RepoIndex {
    pub display: BTreeMap<String, String>,
    pub ids: BTreeMap<String, AgentRepositoryId>,
}

impl PackageCatalogue {
    pub(crate) async fn repo_index(&self) -> Result<RepoIndex, PackageError> {
        let repositories = self
            .inner
            .ports
            .catalogue
            .list_repositories()
            .await
            .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
        let mut index = RepoIndex::default();
        index
            .display
            .insert("bundled".to_owned(), "ordin".to_owned());
        index
            .ids
            .insert("bundled".to_owned(), BUNDLED_REPOSITORY_ID);
        for summary in repositories {
            let repository = summary.repository;
            if repository.id == BUNDLED_REPOSITORY_ID {
                continue;
            }
            let label = format!("{}/{}", repository.host, repository.repository);
            index.display.insert(label.clone(), repository.display_name);
            index.ids.insert(label, repository.id);
        }
        Ok(index)
    }

    /// §A0.4: one `UsesItem` per selected pack and overlay.
    pub(crate) async fn uses_of(&self, plan: &ResolutionPlan, repos: &RepoIndex) -> Vec<UsesItem> {
        let mut items = Vec::new();
        let selected = plan
            .selections
            .practice_packs
            .iter()
            .chain(plan.selections.overlays.iter());
        for choice in selected {
            let Some(part) = plan
                .resolved
                .iter()
                .chain(plan.overlay_order.iter())
                .chain(plan.practice_packs.iter())
                .find(|p| p.id == choice.id)
            else {
                continue;
            };
            let label = match repos.ids.get(&part.repository) {
                Some(repository) => self
                    .inner
                    .ports
                    .catalogue
                    .find_part(*repository, part.kind, &part.id, Some(&part.version))
                    .await
                    .map(|view| view.part.name)
                    .unwrap_or_else(|_| choice.id.clone()),
                None => choice.id.clone(),
            };
            items.push(UsesItem {
                kind: part.kind,
                identifier: choice.id.clone(),
                label,
                chosen_by: choice.chosen_by,
                changeable: choice.chosen_by != ChosenBy::MakerFixed,
            });
        }
        items
    }

    /// §A0.5: the Package Inspector's request for an agent folder and a bundle.
    pub(crate) async fn inspect_request(
        &self,
        id: AgentInstanceId,
        folder: &Path,
        bundle: &ordin_resolver::Bundle,
    ) -> Result<InspectRequest, PackageError> {
        let repos = self.repo_index().await?;
        let mut part_states = BTreeMap::new();
        for part in &bundle.plan.resolved {
            let Some(repository) = repos.ids.get(&part.repository) else {
                continue;
            };
            let view = self
                .inner
                .ports
                .catalogue
                .find_part(*repository, part.kind, &part.id, Some(&part.version))
                .await
                .map_err(|e| PackageError::library_unavailable(e.retryable, e.code))?;
            let state = match view.state_label {
                PartStateLabel::Current | PartStateLabel::NotOffered => PartState::InUse,
                PartStateLabel::NotForNewAgents => PartState::NotForNewAgents,
                PartStateLabel::Withdrawn => PartState::Withdrawn,
            };
            part_states.insert(part.id.clone(), state);
        }
        let record = self
            .inner
            .ports
            .installers
            .record(id)
            .await
            .map_err(|e| PackageError::install_failed(e.code()))?;
        let plugin_name = match &record.launch {
            HarnessLaunch::ClaudeCode { plugin_name, .. } => Some(plugin_name.clone()),
            HarnessLaunch::Copilot { .. } => None,
        };
        let facts = InstallFacts {
            plugin_name,
            command_names: record
                .commands
                .iter()
                .map(|c| (c.command_id.clone(), c.session_name.clone()))
                .collect(),
            reference_only_parts: record.reference_only_parts.clone(),
            needs_person_present: record.needs_person_present,
        };
        Ok(InspectRequest {
            agent_instance_id: id,
            folder_path: folder.to_path_buf(),
            bundle_dir: Some(bundle.dir.clone()),
            repository_names: repos.display,
            part_states,
            install: Some(facts),
        })
    }
}

/// The kind a selected id has in a plan (`PracticePack` for packs), used by the answer check.
pub(crate) fn selected_by(plan: &ResolutionPlan, pack: &str) -> Option<ChosenBy> {
    plan.selections
        .practice_packs
        .iter()
        .find(|s| s.id == pack)
        .map(|s| s.chosen_by)
}
