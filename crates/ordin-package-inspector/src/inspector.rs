//! `PackageInspector`: the public operations (PackageInspector.md *Public Interface*), the store of
//! models, rule sets and workspace memberships (*Data Model*) and the directory with its changes.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::stream::BoxStream;
use futures::StreamExt;
use ordin_agent_model::{canonical_json, MemoryContract, PartKind};
use ordin_format_plugins::FormatPlugins;
use ordin_rules::{AgentToolPolicy, ControlSource, InstanceRuleSet, RulesEngine, ToolDescription};
use ordin_types::{AgentInstanceId, WorkspaceId};
use tokio::sync::broadcast;

use crate::derive::{derive, Built};
use crate::error::InspectError;
use crate::fsio::{atomic_write, sha256_hex};
use crate::may_do::may_do;
use crate::model::{
    is_ability, AgentDirectoryEntry, Detail, InspectOutcome, InspectRequest, InstalledFileEntry,
    InstalledFileText, InstalledMadeOf, ModelSource, PackageModel, PartPage, RuleSetReloadFailure,
    MODEL_SCHEMA,
};
use crate::{views, ToolKind};

const MEMBERSHIPS_FILE: &str = "memberships.json";
const RULES_SUFFIX: &str = ".rules.json";
/// Capacity of the in-process change channel (step 16).
const CHANGE_CAPACITY: usize = 256;

/// The Package Inspector. Cheap to clone: the state is shared.
#[derive(Clone)]
pub struct PackageInspector {
    inner: Arc<Inner>,
}

struct Inner {
    rules: Arc<RulesEngine>,
    plugins: FormatPlugins,
    store_dir: PathBuf,
    changes: broadcast::Sender<WorkspaceId>,
    /// Serialises read-modify-write cycles of `memberships.json`.
    memberships_lock: Mutex<()>,
}

impl PackageInspector {
    /// Called by: Ordin Desktop and Ordin Server at start. Creates the model store when absent.
    pub fn new(rules: Arc<RulesEngine>, store_dir: PathBuf) -> Result<Self, InspectError> {
        std::fs::create_dir_all(&store_dir).map_err(|_| InspectError::write_failed())?;
        let (changes, _) = broadcast::channel(CHANGE_CAPACITY);
        Ok(PackageInspector {
            inner: Arc::new(Inner {
                rules,
                plugins: FormatPlugins::builtin(),
                store_dir,
                changes,
                memberships_lock: Mutex::new(()),
            }),
        })
    }

    /// Runs blocking file work off the async runtime.
    async fn run<T, F>(&self, work: F) -> Result<T, InspectError>
    where
        T: Send + 'static,
        F: FnOnce(&Inner) -> Result<T, InspectError> + Send + 'static,
    {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || work(&inner))
            .await
            .map_err(|_| InspectError::write_failed())?
    }

    pub async fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError> {
        let request = request.clone();
        self.run(move |inner| inner.inspect(&request)).await
    }

    /// Inspects an update candidate in its bundle folder: steps 1 to 6 without loading the rule set
    /// and without storing anything; the installed model is untouched.
    pub async fn inspect_candidate(
        &self,
        request: &InspectRequest,
    ) -> Result<PackageModel, InspectError> {
        let request = request.clone();
        self.run(move |inner| inner.inspect_candidate(&request))
            .await
    }

    pub async fn model(&self, id: AgentInstanceId) -> Result<PackageModel, InspectError> {
        self.run(move |inner| inner.model(id)).await
    }

    pub async fn made_of(
        &self,
        id: AgentInstanceId,
        detail: Detail,
    ) -> Result<InstalledMadeOf, InspectError> {
        self.run(move |inner| Ok(views::made_of(&inner.model(id)?, detail)))
            .await
    }

    /// The agent folder's files, `ordin.lock` first, then by path; LinksCode only (ADR-093 item 3).
    pub async fn installed_files(
        &self,
        id: AgentInstanceId,
        folder_path: &Path,
    ) -> Result<Vec<InstalledFileEntry>, InspectError> {
        let folder = folder_path.to_path_buf();
        self.run(move |_| {
            let _ = id;
            views::installed_files(&folder)
        })
        .await
    }

    /// One file of the agent folder, read only; a path that leaves the folder is refused.
    pub async fn installed_file(
        &self,
        id: AgentInstanceId,
        folder_path: &Path,
        path: &str,
    ) -> Result<InstalledFileText, InspectError> {
        let folder = folder_path.to_path_buf();
        let path = path.to_owned();
        self.run(move |_| {
            let _ = id;
            views::installed_file(&folder, &path)
        })
        .await
    }

    pub async fn part_page(
        &self,
        id: AgentInstanceId,
        part_id: &str,
        bundle_dir: &Path,
        detail: Detail,
    ) -> Result<PartPage, InspectError> {
        let part_id = part_id.to_owned();
        let bundle_dir = bundle_dir.to_path_buf();
        self.run(move |inner| views::part_page(&inner.model(id)?, &part_id, &bundle_dir, detail))
            .await
    }

    /// Deletes the stored model and rule set and unloads the rule set (instance removed).
    pub async fn forget(&self, id: AgentInstanceId) -> Result<(), InspectError> {
        self.run(move |inner| inner.forget(id)).await
    }

    /// Loads every stored rule set into the Rules Engine (program start); returns the failures.
    pub async fn reload_rule_sets(&self) -> Result<Vec<RuleSetReloadFailure>, InspectError> {
        self.run(|inner| inner.reload_rule_sets()).await
    }

    /// Records the workspaces an instance belongs to, for `directory`; empty removes it from all.
    pub async fn set_workspaces(
        &self,
        id: AgentInstanceId,
        workspace_ids: Vec<WorkspaceId>,
    ) -> Result<(), InspectError> {
        self.run(move |inner| inner.set_workspaces(id, workspace_ids))
            .await
    }

    /// The installed agents of one workspace, ordered by `display_name` then instance id.
    pub async fn directory(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<AgentDirectoryEntry>, InspectError> {
        self.run(move |inner| inner.directory(workspace_id)).await
    }

    /// One item per workspace whose directory changed (step 16). A subscriber that falls behind
    /// receives every workspace id in `memberships.json` once, so it rebuilds every directory.
    pub fn changes(&self) -> BoxStream<'static, WorkspaceId> {
        let receiver = self.inner.changes.subscribe();
        let store_dir = self.inner.store_dir.clone();
        futures::stream::unfold(
            (receiver, VecDeque::<WorkspaceId>::new(), store_dir),
            |(mut receiver, mut pending, store_dir)| async move {
                loop {
                    if let Some(workspace) = pending.pop_front() {
                        return Some((workspace, (receiver, pending, store_dir)));
                    }
                    match receiver.recv().await {
                        Ok(workspace) => return Some((workspace, (receiver, pending, store_dir))),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            let everything = read_memberships(&store_dir)
                                .map(|map| all_workspaces(&map))
                                .unwrap_or_default();
                            pending.extend(everything);
                        }
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
            },
        )
        .boxed()
    }

    /// The installed agent's Memory part contract; None when it has none (step 17, SA-CLOSE-05).
    pub async fn memory_contract(
        &self,
        id: AgentInstanceId,
    ) -> Result<Option<MemoryContract>, InspectError> {
        // spec: ordin-desktop/components/PackageInspector#SA-CLOSE-05
        Ok(self.model(id).await?.memory)
    }
}

type Memberships = BTreeMap<String, BTreeSet<String>>;

fn read_memberships(store_dir: &Path) -> Result<Memberships, InspectError> {
    match std::fs::read(store_dir.join(MEMBERSHIPS_FILE)) {
        Ok(bytes) => serde_json::from_slice::<Memberships>(&bytes)
            .map_err(|_| InspectError::stored_unusable()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Memberships::new()),
        Err(_) => Err(InspectError::stored_unusable()),
    }
}

fn parse_workspaces(ids: &BTreeSet<String>) -> Vec<WorkspaceId> {
    ids.iter()
        .filter_map(|id| WorkspaceId::parse(id).ok())
        .collect()
}

fn all_workspaces(map: &Memberships) -> Vec<WorkspaceId> {
    let every: BTreeSet<&String> = map.values().flatten().collect();
    every
        .into_iter()
        .filter_map(|id| WorkspaceId::parse(id).ok())
        .collect()
}

/// Canonical JSON of any serialisable value (sorted keys).
fn canonical<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, InspectError> {
    serde_json::to_value(value)
        .map(|value| canonical_json(&value))
        .map_err(|_| InspectError::write_failed())
}

impl Inner {
    fn model_path(&self, id: AgentInstanceId) -> PathBuf {
        self.store_dir.join(format!("{id}.json"))
    }

    fn rules_path(&self, id: AgentInstanceId) -> PathBuf {
        self.store_dir.join(format!("{id}{RULES_SUFFIX}"))
    }

    fn notify(&self, workspaces: impl IntoIterator<Item = WorkspaceId>) {
        for workspace in workspaces {
            // A send fails only when nobody is subscribed, which needs no action.
            let _ = self.changes.send(workspace);
        }
    }

    fn workspaces_of(&self, id: AgentInstanceId) -> Vec<WorkspaceId> {
        read_memberships(&self.store_dir)
            .ok()
            .and_then(|map| map.get(&id.to_string()).map(parse_workspaces))
            .unwrap_or_default()
    }

    fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError> {
        let started = Instant::now();
        let result = self.inspect_inner(request, started);
        if let Err(error) = &result {
            tracing::warn!(
                agent_instance_id = request.agent_instance_id.to_string().as_str(),
                error_code = error.code(),
                "package_inspector.inspect_failed"
            );
        }
        result
    }

    fn inspect_inner(
        &self,
        request: &InspectRequest,
        started: Instant,
    ) -> Result<InspectOutcome, InspectError> {
        let id = request.agent_instance_id;
        let mut built = derive(&self.plugins, request)?;
        // Step 6a: the rule set, handed to the Rules Engine.
        let set = rule_set_of(request, &built);
        let lines = self.rules.load_rule_set(set.clone()).map_err(|error| {
            InspectError::DeclarationRefused {
                code: error.code(),
                retryable: error.retryable(),
            }
        })?;
        built.model.declared_rules = lines
            .iter()
            .filter(|line| line.source == ControlSource::Package)
            .map(|line| line.reason.clone())
            .collect();
        atomic_write(&self.rules_path(id), &canonical(&set)?)
            .map_err(|_| InspectError::write_failed())?;
        // Step 7: the store.
        let bytes = canonical(&built.model)?;
        atomic_write(&self.model_path(id), &bytes).map_err(|_| InspectError::write_failed())?;
        let reference = format!("pki:{id}:{}", sha256_hex(&bytes));
        let outcome = InspectOutcome {
            may_do: may_do(&built.model),
            parsed_declaration_ref: reference,
            model: built.model,
        };
        self.notify(self.workspaces_of(id));
        let model = &outcome.model;
        tracing::info!(
            agent_instance_id = id.to_string().as_str(),
            source = match model.source {
                ModelSource::OrdinBundle { .. } => "ordin_bundle",
                ModelSource::ApmFolder { .. } => "apm_folder",
            },
            parts = model.parts.len(),
            tasks = model.tasks.len(),
            tools = model.tools.len(),
            connections = model.connections.len(),
            authority = ordin_types::log::wire_word(&model.authority).as_str(),
            authority_source = ordin_types::log::wire_word(&model.authority_source).as_str(),
            not_carried = model.not_carried.len(),
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "package_inspector.inspected"
        );
        Ok(outcome)
    }

    /// Step 13.
    fn inspect_candidate(&self, request: &InspectRequest) -> Result<PackageModel, InspectError> {
        let started = Instant::now();
        let built = derive(&self.plugins, request).inspect_err(|error| {
            tracing::warn!(
                agent_instance_id = request.agent_instance_id.to_string().as_str(),
                error_code = error.code(),
                "package_inspector.inspect_failed"
            );
        })?;
        tracing::info!(
            agent_instance_id = request.agent_instance_id.to_string().as_str(),
            duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "package_inspector.candidate_inspected"
        );
        Ok(built.model)
    }

    /// Step 8.
    fn model(&self, id: AgentInstanceId) -> Result<PackageModel, InspectError> {
        let bytes = match std::fs::read(self.model_path(id)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(InspectError::NotInspected)
            }
            Err(_) => return Err(InspectError::stored_unusable()),
        };
        // Another schema is `PKI_STORE_FAILED { retryable: false }`; the Package Catalogue re-runs
        // `inspect`.
        let model: PackageModel =
            serde_json::from_slice(&bytes).map_err(|_| InspectError::stored_unusable())?;
        if model.model_schema != MODEL_SCHEMA {
            return Err(InspectError::stored_unusable());
        }
        Ok(model)
    }

    /// Step 12.
    fn forget(&self, id: AgentInstanceId) -> Result<(), InspectError> {
        for path in [self.model_path(id), self.rules_path(id)] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(InspectError::write_failed()),
            }
        }
        self.set_workspaces(id, Vec::new())?;
        self.rules.unload_rule_set(id);
        Ok(())
    }

    /// Step 12a.
    fn reload_rule_sets(&self) -> Result<Vec<RuleSetReloadFailure>, InspectError> {
        let listing =
            std::fs::read_dir(&self.store_dir).map_err(|_| InspectError::stored_unusable())?;
        let mut names: Vec<String> = listing
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(RULES_SUFFIX))
            .collect();
        names.sort();
        let mut failures = Vec::new();
        for name in names {
            let stem = name.trim_end_matches(RULES_SUFFIX);
            let Ok(id) = AgentInstanceId::parse(stem) else {
                continue;
            };
            let code = match std::fs::read(self.store_dir.join(&name))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<InstanceRuleSet>(&bytes).ok())
            {
                None => Some("PKI_STORE_FAILED".to_owned()),
                Some(set) => self
                    .rules
                    .load_rule_set(set)
                    .err()
                    .map(|error| error.code().to_owned()),
            };
            if let Some(code) = code {
                tracing::warn!(
                    agent_instance_id = id.to_string().as_str(),
                    error_code = code.as_str(),
                    "package_inspector.rule_set_reload_failed"
                );
                failures.push(RuleSetReloadFailure {
                    agent_instance_id: id,
                    code,
                });
            }
        }
        Ok(failures)
    }

    /// Step 16: `set_workspaces`.
    fn set_workspaces(
        &self,
        id: AgentInstanceId,
        workspace_ids: Vec<WorkspaceId>,
    ) -> Result<(), InspectError> {
        let _guard = self
            .memberships_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut map = read_memberships(&self.store_dir)?;
        let key = id.to_string();
        let old: Vec<WorkspaceId> = map.get(&key).map(parse_workspaces).unwrap_or_default();
        if workspace_ids.is_empty() {
            map.remove(&key);
        } else {
            map.insert(key, workspace_ids.iter().map(ToString::to_string).collect());
        }
        atomic_write(&self.store_dir.join(MEMBERSHIPS_FILE), &canonical(&map)?)
            .map_err(|_| InspectError::write_failed())?;
        let touched: BTreeSet<WorkspaceId> = old.into_iter().chain(workspace_ids).collect();
        self.notify(touched);
        Ok(())
    }

    /// Step 16: `directory`.
    fn directory(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<AgentDirectoryEntry>, InspectError> {
        let map = read_memberships(&self.store_dir)?;
        let wanted = workspace_id.to_string();
        let mut entries = Vec::new();
        for (instance, workspaces) in &map {
            if !workspaces.contains(&wanted) {
                continue;
            }
            let Ok(id) = AgentInstanceId::parse(instance) else {
                continue;
            };
            let model = match self.model(id) {
                Ok(model) => model,
                Err(InspectError::NotInspected) => {
                    tracing::warn!(
                        agent_instance_id = id.to_string().as_str(),
                        workspace_id = workspace_id.to_string().as_str(),
                        "package_inspector.directory_missing_model"
                    );
                    continue;
                }
                Err(error) => return Err(error),
            };
            entries.push(entry_of(&model));
        }
        entries.sort_by(|a, b| {
            a.display_name
                .as_bytes()
                .cmp(b.display_name.as_bytes())
                .then_with(|| a.agent_instance_id.cmp(&b.agent_instance_id))
        });
        Ok(entries)
    }
}

fn entry_of(model: &PackageModel) -> AgentDirectoryEntry {
    let mut skills: Vec<String> = model
        .parts
        .iter()
        .filter(|line| is_ability(line.part.kind))
        .map(|line| line.name.clone())
        .collect();
    skills.sort();
    AgentDirectoryEntry {
        agent_instance_id: model.agent_instance_id,
        display_name: model.display_name.clone(),
        description: model.description.clone(),
        plugin_name: model.plugin_name.clone(),
        tasks: model.tasks.clone(),
        skills,
        lifecycle: model
            .parts
            .iter()
            .find(|line| line.part.kind == PartKind::Journey)
            .map(|line| line.name.clone()),
        authority: model.authority,
        declared_rules: model.declared_rules.clone(),
        needs_person_present: model.needs_person_present,
    }
}

/// Step 6a: the instance's rule set, from the derived model, the declaration and the tool policy.
// spec: ordin-desktop/components/PackageInspector#SA-RECON-01
fn rule_set_of(request: &InspectRequest, built: &Built) -> InstanceRuleSet {
    let tools = &built.model.tools;
    // spec: ordin-desktop/components/PackageInspector#SA-CLOSE-03
    let mut approval_list: Vec<String> = Vec::new();
    if let Some(declaration) = &built.declaration {
        for name in &declaration.approval_required {
            if !approval_list.contains(name) {
                approval_list.push(name.clone());
            }
        }
    }
    InstanceRuleSet {
        agent_instance_id: request.agent_instance_id,
        package_name: built.package_name.clone(),
        authority: built.model.authority,
        agent_folder: request.folder_path.clone(),
        agent_folder_read_only: false,
        declaration: built.declaration.clone(),
        configured: None,
        tool_policy: built.tool_policy.as_ref().map(|policy| AgentToolPolicy {
            prohibited_verbs: policy.prohibited_verbs.clone(),
            forbidden_flags: policy.forbidden_flags.clone(),
            requires_approval: policy.requires_human_approval.clone(),
            mcp_servers: policy.mcp_servers.iter().map(|s| s.name.clone()).collect(),
        }),
        approval_list,
        package_read_only_tools: tools
            .iter()
            .filter(|tool| tool.kind == ToolKind::McpServer && tool.permission == "read-only")
            .map(|tool| tool.name.clone())
            .collect(),
        // spec: ordin-desktop/components/PackageInspector#SA-CLOSE-02
        tools: tools
            .iter()
            .map(|tool| ToolDescription {
                name: tool.name.clone(),
                arguments: tool.arguments.clone(),
                served_by: None,
            })
            .collect(),
        unmapped_operations: Vec::new(),
        external: Vec::new(),
        commands: built.commands.clone(),
        reviewer: None,
    }
}
