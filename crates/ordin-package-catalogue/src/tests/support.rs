//! Test support: a real SQLite datastore with the Journal and this crate's migration, and a double
//! for each of the six providers. A double implements the private port trait with exactly the
//! provider's signatures and records what it was asked, in one log shared by all of them.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ordin_agent_folders::{AfmError, AgentFolder};
use ordin_catalogue::{
    AgentRepositoryRef, CatalogueError, CatalogueNotice, LibraryEntry, LibraryQuery,
    PartStateLabel, PartView, RepositorySummary,
};
use ordin_checker::{
    CheckError, CheckReport, CheckRunId, CheckSubject, ExecutablePart, InstallCheck,
    InstallCheckReport, InstallVerdict,
};
use ordin_installers::{
    CommandName, HarnessLaunch, InstallError, InstallOutcome, InstallRecord, InstallRequest,
    InstallerKind, VerifyOutcome,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_package_inspector::{
    AuthoritySource, ConnectionEntry, InspectError, InspectOutcome, InspectRequest, MayDo,
    ModelSource, PackageModel, TaskEntry, ToolEntry, ToolKind, MODEL_SCHEMA,
};
use ordin_resolver::model::{
    BundleManifest, ChosenBy, LockFile, PartFiles, PartKind, PartRef, PartSource, PartSourceError,
    PendingPackChoice, RepositoryRef, Resolution, ResolutionPlan, ResolvedToolPolicy, SelectedPart,
    Selections, SourceScope, SourcedPart, VocabularyAxis, VocabularySet, WithdrawnListEntry,
};
use ordin_resolver::{Bundle, PruneReport, ResolveError, ResolveRequest};
use ordin_types::domain::agents::{
    AgentFormat, AgentRepository, AgentRepositoryState, Authority, DecisionSource,
    ExecutablePartDecision, ExecutablePartKind, ExecutablePartTrust, Part, PartStatus,
    RepositorySource,
};
use ordin_types::domain::HarnessKind;
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, AgentRepositoryId, ExecutablePartDecisionId, Page, PartId, WorkspaceId,
};
use tokio::sync::broadcast;

use crate::catalogue::{HarnessSettings, InstanceActivity, PackageCatalogue};
use crate::ports::{
    CataloguePort, CheckerPort, FoldersPort, InspectorPort, InstallersPort, Ports, ResolverPort,
};
use crate::types::{AddAgentInput, InstalledAgent};

pub type Log = Arc<Mutex<Vec<String>>>;

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "ordin-package-catalogue-{}",
            AgentInstanceId::new()
        ));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The start of an absolute path on this platform: a Windows path needs a drive.
#[cfg(unix)]
pub const ROOT: &str = "/";
#[cfg(windows)]
pub const ROOT: &str = "C:/";

pub const AGENT_IDENTIFIER: &str = "urn:ordin:acme:agents:analyst";
pub const GO_PACK: &str = "urn:ordin:acme:packs:go";
pub const PYTEST_PACK: &str = "urn:ordin:acme:packs:pytest";
pub const UNITTEST_PACK: &str = "urn:ordin:acme:packs:unittest";
pub const COMMIT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const COMMIT_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub const REPO_LABEL: &str = "github.com/acme/agents";

pub fn digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}

pub fn part_ref(kind: PartKind, id: &str, version: &str, commit: &str) -> PartRef {
    PartRef {
        kind,
        id: id.to_owned(),
        name: ordin_resolver::model::slug_from_id(id),
        version: version.to_owned(),
        content_hash: digest(),
        repository: REPO_LABEL.to_owned(),
        commit: commit.to_owned(),
        loading: None,
    }
}

/// A plan of the analyst agent using the Go pack, identified by `hash`.
pub fn plan(hash: &str, commit: &str, version: &str) -> ResolutionPlan {
    let agent = part_ref(
        PartKind::WorkspaceProfile,
        AGENT_IDENTIFIER,
        version,
        commit,
    );
    let pack = part_ref(PartKind::PracticePack, GO_PACK, "1.0.0", commit);
    ResolutionPlan {
        plan_hash: format!("sha256:{hash:0>64}"),
        root: agent.clone(),
        target_harness: "claude_code".to_owned(),
        overlay_order: Vec::new(),
        practice_packs: vec![pack.clone()],
        resolved: vec![agent, pack],
        selections: Selections {
            practice_packs: vec![SelectedPart {
                id: GO_PACK.to_owned(),
                chosen_by: ChosenBy::WorkspaceCode,
            }],
            overlays: Vec::new(),
            pending_choices: Vec::new(),
        },
        warnings: Vec::new(),
        withdrawn_snapshot: digest(),
        tool_policy: None,
    }
}

pub fn pending(rule_index: u32, class: &str, choices: &[&str]) -> PendingPackChoice {
    PendingPackChoice {
        rule_index,
        at_least: 1,
        class: Some(class.to_owned()),
        choices: choices.iter().map(|c| (*c).to_owned()).collect(),
    }
}

pub fn part(
    id: PartId,
    repo: AgentRepositoryId,
    kind: PartKind,
    identifier: &str,
    name: &str,
    version: &str,
) -> Part {
    Part {
        id,
        agent_repository_id: repo,
        kind,
        identifier: identifier.to_owned(),
        name: name.to_owned(),
        version: version.to_owned(),
        status: PartStatus::Approved,
        supersedes: None,
        owner: "acme".to_owned(),
        file_path: "models/part.md".to_owned(),
        commit: COMMIT_A.to_owned(),
        content_digest: Sha256Hex("0".repeat(64)),
        vocabulary_terms_cited: Vec::new(),
        uses: Vec::new(),
        withdrawn: false,
    }
}

pub fn view(part: Part, offered: bool, state: PartStateLabel) -> PartView {
    PartView {
        part,
        summary: "Compares things.".to_owned(),
        offered,
        state_label: state,
        from: "Acme agents".to_owned(),
    }
}

pub fn repository(id: AgentRepositoryId) -> AgentRepository {
    AgentRepository {
        id,
        display_name: "Acme agents".to_owned(),
        host: "github.com".to_owned(),
        repository: "acme/agents".to_owned(),
        protected_branch: "main".to_owned(),
        format: AgentFormat::Ordin,
        format_version: "1".to_owned(),
        source: RepositorySource::Profile,
        signed_merges_required: None,
        person_can_publish: false,
        person_can_curate: false,
        last_fetched_commit: Some(COMMIT_A.to_owned()),
        clone_path: "/clones/acme".to_owned(),
        state: AgentRepositoryState::Cloned,
        added_at: Timestamp::now(),
    }
}

// ---- the Catalogue Service ----

/// A part source with nothing in it; a draft source in a trial.
pub struct EmptySource;

#[async_trait]
impl PartSource for EmptySource {
    async fn resolve(
        &self,
        _: &SourceScope,
        _: PartKind,
        _: &str,
        _: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        Ok(Resolution::NotFound)
    }
    async fn resolve_any_kind(
        &self,
        _: &SourceScope,
        _: &str,
    ) -> Result<Resolution, PartSourceError> {
        Ok(Resolution::NotFound)
    }
    async fn files(&self, _: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        Err(PartSourceError::Unavailable { retryable: false })
    }
    async fn ingredients(&self, _: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        Ok(Vec::new())
    }
    async fn withdrawn(&self, _: &SourceScope) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        Ok(Vec::new())
    }
    async fn repository_parts(
        &self,
        _: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        Ok(Vec::new())
    }
    async fn vocabulary(&self, _: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        Err(PartSourceError::Unavailable { retryable: false })
    }
    async fn used_by(&self, _: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        Ok(Vec::new())
    }
    async fn citing_parts(
        &self,
        _: &RepositoryRef,
        _: VocabularyAxis,
        _: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        Ok(Vec::new())
    }
}

pub fn catalogue_error(code: &'static str, retryable: bool) -> CatalogueError {
    CatalogueError {
        code,
        retryable,
        action: None,
        detail: String::new(),
    }
}

pub struct FakeCatalogue {
    pub repo: AgentRepositoryId,
    pub parts: Mutex<Vec<PartView>>,
    pub part_error: Mutex<Option<(&'static str, bool)>>,
    pub find_error: Mutex<Option<(&'static str, bool)>>,
    pub scope_error: Mutex<Option<(&'static str, bool)>>,
    pub list_error: Mutex<Option<(&'static str, bool)>>,
    pub search_error: Mutex<Option<(&'static str, bool)>>,
    pub by_ref: Mutex<Result<Option<AgentRepository>, (&'static str, bool)>>,
    pub repo_ref: Mutex<AgentRepositoryRef>,
    pub repo_ref_error: Mutex<Option<(&'static str, bool)>>,
    pub entries: Mutex<Vec<LibraryEntry>>,
    pub withdrawn: Mutex<Vec<WithdrawnListEntry>>,
    pub withdrawn_error: Mutex<bool>,
    pub draft_error: Mutex<Option<(&'static str, bool)>>,
    pub tx: broadcast::Sender<CatalogueNotice>,
    pub log: Log,
}

impl FakeCatalogue {
    pub fn new(repo: AgentRepositoryId, log: Log) -> Arc<FakeCatalogue> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(FakeCatalogue {
            repo,
            parts: Mutex::new(Vec::new()),
            part_error: Mutex::new(None),
            find_error: Mutex::new(None),
            scope_error: Mutex::new(None),
            list_error: Mutex::new(None),
            search_error: Mutex::new(None),
            by_ref: Mutex::new(Ok(Some(repository(repo)))),
            repo_ref: Mutex::new(AgentRepositoryRef::ProfileKey("acme-agents".to_owned())),
            repo_ref_error: Mutex::new(None),
            entries: Mutex::new(Vec::new()),
            withdrawn: Mutex::new(Vec::new()),
            withdrawn_error: Mutex::new(false),
            draft_error: Mutex::new(None),
            tx,
            log,
        })
    }

    fn fail(slot: &Mutex<Option<(&'static str, bool)>>) -> Result<(), CatalogueError> {
        match *lock(slot) {
            Some((code, retryable)) => Err(catalogue_error(code, retryable)),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl CataloguePort for FakeCatalogue {
    fn subscribe(&self) -> broadcast::Receiver<CatalogueNotice> {
        self.tx.subscribe()
    }
    async fn part(&self, id: PartId) -> Result<PartView, CatalogueError> {
        Self::fail(&self.part_error)?;
        lock(&self.parts)
            .iter()
            .find(|v| v.part.id == id)
            .cloned()
            .ok_or_else(|| catalogue_error("CAT_PART_UNKNOWN", false))
    }
    async fn find_part(
        &self,
        repository: AgentRepositoryId,
        kind: PartKind,
        identifier: &str,
        version: Option<&str>,
    ) -> Result<PartView, CatalogueError> {
        Self::fail(&self.find_error)?;
        let parts = lock(&self.parts);
        let named = |v: &&PartView| {
            v.part.agent_repository_id == repository
                && v.part.kind == kind
                && v.part.identifier == identifier
        };
        // The exact version when the library holds it, otherwise the head version of the part.
        parts
            .iter()
            .filter(named)
            .find(|v| version.is_none_or(|ver| v.part.version == ver))
            .or_else(|| parts.iter().find(named))
            .cloned()
            .ok_or_else(|| catalogue_error("CAT_PART_UNKNOWN", false))
    }
    async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError> {
        Self::fail(&self.search_error)?;
        lock(&self.log).push(format!("search:{}", query.text.unwrap_or_default()));
        Ok(Page {
            items: lock(&self.entries).clone(),
            next_cursor: None,
        })
    }
    async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError> {
        Self::fail(&self.list_error)?;
        Ok(vec![RepositorySummary {
            repository: repository(self.repo),
            agent_count: 1,
            set_by_company: true,
            last_fetch: None,
        }])
    }
    async fn source_scope(&self, home: RepositoryRef) -> Result<SourceScope, CatalogueError> {
        Self::fail(&self.scope_error)?;
        Ok(SourceScope {
            home,
            profile: Vec::new(),
            include_bundled: true,
        })
    }
    async fn repository_by_ref(
        &self,
        _: &AgentRepositoryRef,
    ) -> Result<Option<AgentRepository>, CatalogueError> {
        lock(&self.by_ref)
            .clone()
            .map_err(|(c, r)| catalogue_error(c, r))
    }
    async fn repository_ref(
        &self,
        _: AgentRepositoryId,
    ) -> Result<AgentRepositoryRef, CatalogueError> {
        Self::fail(&self.repo_ref_error)?;
        Ok(lock(&self.repo_ref).clone())
    }
    async fn draft_source(
        &self,
        _: AgentRepositoryId,
        worktree: PathBuf,
    ) -> Result<Arc<dyn PartSource>, CatalogueError> {
        Self::fail(&self.draft_error)?;
        lock(&self.log).push(format!("draft_source:{}", worktree.display()));
        Ok(Arc::new(EmptySource))
    }
    async fn withdrawn(&self, _: &SourceScope) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        if *lock(&self.withdrawn_error) {
            return Err(PartSourceError::Unavailable { retryable: false });
        }
        Ok(lock(&self.withdrawn).clone())
    }
}

// ---- the Resolver and Compiler ----

pub type PlanFn =
    Arc<dyn Fn(&ResolveRequest) -> Result<ResolutionPlan, ResolveError> + Send + Sync>;

pub struct FakeResolver {
    pub dir: PathBuf,
    pub plan: Mutex<ResolutionPlan>,
    pub plan_fn: Mutex<Option<PlanFn>>,
    pub resolve_error: Mutex<Option<ResolveError>>,
    pub build_error: Mutex<Option<ResolveError>>,
    pub build_failures: AtomicU32,
    pub requests: Mutex<Vec<ResolveRequest>>,
    pub bundles: Mutex<HashMap<String, Bundle>>,
    pub pruned: Mutex<Vec<std::collections::BTreeSet<String>>>,
    pub prune_error: Mutex<bool>,
    pub open_error: Mutex<bool>,
    pub log: Log,
}

impl FakeResolver {
    pub fn new(dir: PathBuf, plan: ResolutionPlan, log: Log) -> Arc<FakeResolver> {
        Arc::new(FakeResolver {
            dir,
            plan: Mutex::new(plan),
            plan_fn: Mutex::new(None),
            resolve_error: Mutex::new(None),
            build_error: Mutex::new(None),
            build_failures: AtomicU32::new(0),
            requests: Mutex::new(Vec::new()),
            bundles: Mutex::new(HashMap::new()),
            pruned: Mutex::new(Vec::new()),
            prune_error: Mutex::new(false),
            open_error: Mutex::new(false),
            log,
        })
    }

    fn planned(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError> {
        lock(&self.requests).push(request.clone());
        if let Some(error) = lock(&self.resolve_error).clone() {
            return Err(error);
        }
        if let Some(f) = lock(&self.plan_fn).clone() {
            return f(request);
        }
        Ok(lock(&self.plan).clone())
    }

    pub fn bundle_of(&self, plan: ResolutionPlan) -> Bundle {
        let hex = plan.plan_hash.trim_start_matches("sha256:").to_owned();
        Bundle {
            manifest: BundleManifest {
                bundle_schema: "ordin.bundle/v1".to_owned(),
                plan_hash: plan.plan_hash.clone(),
                composition: plan.root.clone(),
                artefacts: Vec::new(),
                overlays_applied: Vec::new(),
                resolved_tool_policy: ResolvedToolPolicy::default(),
                lockfile_sha256: "1".repeat(64),
                compiler_version: "0.1.0-test".to_owned(),
            },
            lock_file: LockFile {
                lockfile_schema: "ordin.lock/v1".to_owned(),
                plan_hash: plan.plan_hash.clone(),
                target_harness: plan.target_harness.clone(),
                composition: plan.root.clone(),
                artefacts: plan.resolved.clone(),
                overlays_applied: Vec::new(),
                selections: plan.selections.clone(),
            },
            lock_file_bytes: Vec::new(),
            dir: self.dir.join("bundles").join(hex),
            plan,
        }
    }

    /// Makes `plan` openable by hash, as a bundle the real Resolver already wrote.
    pub fn register(&self, plan: ResolutionPlan) {
        let bundle = self.bundle_of(plan.clone());
        lock(&self.bundles).insert(plan.plan_hash, bundle);
    }
}

#[async_trait]
impl ResolverPort for FakeResolver {
    async fn resolve(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError> {
        lock(&self.log).push("resolver.resolve".to_owned());
        self.planned(request)
    }
    async fn build(&self, request: &ResolveRequest) -> Result<Bundle, ResolveError> {
        lock(&self.log).push("resolver.build".to_owned());
        if self
            .build_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(ResolveError::StoreWriteFailed { retryable: true });
        }
        if let Some(error) = lock(&self.build_error).clone() {
            return Err(error);
        }
        let plan = self.planned(request)?;
        let bundle = self.bundle_of(plan.clone());
        lock(&self.bundles).insert(plan.plan_hash, bundle.clone());
        Ok(bundle)
    }
    fn prune(
        &self,
        keep: &std::collections::BTreeSet<String>,
    ) -> Result<PruneReport, ResolveError> {
        lock(&self.pruned).push(keep.clone());
        if *lock(&self.prune_error) {
            return Err(ResolveError::StoreWriteFailed { retryable: true });
        }
        Ok(PruneReport::default())
    }
    fn open_bundle(&self, plan_hash: &str) -> Result<Bundle, ResolveError> {
        if *lock(&self.open_error) {
            return Err(ResolveError::BundleNotFound {
                plan_hash: plan_hash.to_owned(),
            });
        }
        lock(&self.bundles)
            .get(plan_hash)
            .cloned()
            .ok_or_else(|| ResolveError::BundleNotFound {
                plan_hash: plan_hash.to_owned(),
            })
    }
}

// ---- the Checker ----

pub struct FakeChecker {
    pub verdict: Mutex<InstallVerdict>,
    pub check_error: Mutex<Option<CheckError>>,
    pub checks: Mutex<Vec<InstallCheck>>,
    pub parts: Mutex<Vec<ExecutablePart>>,
    pub parts_error: Mutex<Option<CheckError>>,
    pub decide_error: Mutex<Option<CheckError>>,
    pub decided: Mutex<Vec<(AgentInstanceId, Vec<ExecutablePart>)>>,
    pub forget_error: Mutex<Option<CheckError>>,
    pub log: Log,
}

pub fn exec_part(name: &str) -> ExecutablePart {
    ExecutablePart {
        part_name: name.to_owned(),
        kind: ExecutablePartKind::McpServer,
        declared_by: format!("urn:ordin:acme:tools:{name}"),
        bundled: false,
    }
}

impl FakeChecker {
    pub fn new(log: Log) -> Arc<FakeChecker> {
        Arc::new(FakeChecker {
            verdict: Mutex::new(InstallVerdict::Clean),
            check_error: Mutex::new(None),
            checks: Mutex::new(Vec::new()),
            parts: Mutex::new(vec![exec_part("files"), exec_part("jira-reader")]),
            parts_error: Mutex::new(None),
            decide_error: Mutex::new(None),
            decided: Mutex::new(Vec::new()),
            forget_error: Mutex::new(None),
            log,
        })
    }
}

#[async_trait]
impl CheckerPort for FakeChecker {
    async fn check_for_install(
        &self,
        request: &InstallCheck,
    ) -> Result<InstallCheckReport, CheckError> {
        lock(&self.log).push("checker.check_for_install".to_owned());
        lock(&self.checks).push(request.clone());
        if let Some(error) = lock(&self.check_error).clone() {
            return Err(error);
        }
        let verdict = lock(&self.verdict).clone();
        Ok(InstallCheckReport {
            report: CheckReport {
                check_run_id: CheckRunId(uuid::Uuid::now_v7()),
                subject: CheckSubject::RepositoryRead {
                    repository: REPO_LABEL.to_owned(),
                    commit: COMMIT_A.to_owned(),
                },
                passed: matches!(verdict, InstallVerdict::Clean),
                findings: Vec::new(),
                tidied: Vec::new(),
                checked_at: Timestamp::now(),
                version_checked: "0.1.0".to_owned(),
                agents_that_stop: Vec::new(),
                publish_gate_met: true,
            },
            verdict,
        })
    }
    async fn executable_parts(
        &self,
        _: AgentInstanceId,
    ) -> Result<Vec<ExecutablePart>, CheckError> {
        lock(&self.log).push("checker.executable_parts".to_owned());
        if let Some(error) = lock(&self.parts_error).clone() {
            return Err(error);
        }
        Ok(lock(&self.parts).clone())
    }
    async fn decide_executable_parts(
        &self,
        id: AgentInstanceId,
        parts: &[ExecutablePart],
    ) -> Result<Vec<ExecutablePartDecision>, CheckError> {
        lock(&self.log).push("checker.decide_executable_parts".to_owned());
        if let Some(error) = lock(&self.decide_error).clone() {
            return Err(error);
        }
        lock(&self.decided).push((id, parts.to_vec()));
        Ok(parts
            .iter()
            .map(|p| ExecutablePartDecision {
                id: ExecutablePartDecisionId::new(),
                agent_instance_id: id,
                part_name: p.part_name.clone(),
                kind: p.kind,
                decision: ExecutablePartTrust::Trusted,
                source: DecisionSource::OrganisationPolicy,
                decided_at: Timestamp::now(),
            })
            .collect())
    }
    async fn forget_instance(&self, _: AgentInstanceId) -> Result<(), CheckError> {
        lock(&self.log).push("checker.forget_instance".to_owned());
        match lock(&self.forget_error).clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

// ---- the Installers ----

pub struct FakeInstallers {
    pub install_failures: AtomicU32,
    pub update_error: Mutex<Option<InstallError>>,
    pub verify: Mutex<Result<VerifyOutcome, InstallError>>,
    pub verify_for: Mutex<HashMap<AgentInstanceId, VerifyOutcome>>,
    pub repair: Mutex<Result<bool, InstallError>>,
    pub uninstall_error: Mutex<Option<InstallError>>,
    pub record_error: Mutex<Option<InstallError>>,
    pub restart_required: Mutex<bool>,
    pub requests: Mutex<Vec<(String, InstallRequest)>>,
    pub log: Log,
}

impl FakeInstallers {
    pub fn new(log: Log) -> Arc<FakeInstallers> {
        Arc::new(FakeInstallers {
            install_failures: AtomicU32::new(0),
            update_error: Mutex::new(None),
            verify: Mutex::new(Ok(VerifyOutcome::Verified)),
            verify_for: Mutex::new(HashMap::new()),
            repair: Mutex::new(Ok(false)),
            uninstall_error: Mutex::new(None),
            record_error: Mutex::new(None),
            restart_required: Mutex::new(false),
            requests: Mutex::new(Vec::new()),
            log,
        })
    }

    pub fn record_of(id: AgentInstanceId, plan_hash: &str) -> InstallRecord {
        InstallRecord {
            record_schema: "ordin.install/v1".to_owned(),
            agent_instance_id: id,
            installer: InstallerKind::ClaudeCode,
            harness: HarnessKind::ClaudeCode,
            plan_hash: plan_hash.to_owned(),
            previous_plan_hash: None,
            lock_file_sha256: "1".repeat(64),
            tool_policy: ResolvedToolPolicy::default(),
            launch: HarnessLaunch::ClaudeCode {
                working_dir: PathBuf::from("/agents/folder"),
                append_system_prompt_file: PathBuf::from("/agents/folder/instructions.md"),
                plugin_dir: PathBuf::from("/agents/folder/plugin"),
                plugin_name: "analyst".to_owned(),
            },
            commands: vec![CommandName {
                command_id: "urn:ordin:acme:commands:plan".to_owned(),
                session_name: "analyst:plan".to_owned(),
            }],
            needs_person_present: false,
            reference_only_parts: Vec::new(),
            files: Vec::new(),
            installed_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        }
    }

    fn outcome(&self, id: AgentInstanceId, hash: &str, repaired: bool) -> InstallOutcome {
        InstallOutcome {
            record: Self::record_of(id, hash),
            restart_required: *lock(&self.restart_required),
            repaired,
        }
    }

    fn note(&self, what: &str, request: &InstallRequest) {
        lock(&self.log).push(format!("installers.{what}"));
        lock(&self.requests).push((what.to_owned(), request.clone()));
    }
}

#[async_trait]
impl InstallersPort for FakeInstallers {
    async fn install(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        self.note("install", request);
        if self
            .install_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(InstallError::WriteFailed { retryable: true });
        }
        Ok(self.outcome(request.agent_instance_id, &request.plan_hash, false))
    }
    async fn update(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        self.note("update", request);
        match lock(&self.update_error).clone() {
            Some(error) => Err(error),
            None => Ok(self.outcome(request.agent_instance_id, &request.plan_hash, false)),
        }
    }
    async fn update_in_session(
        &self,
        request: &InstallRequest,
    ) -> Result<InstallOutcome, InstallError> {
        self.note("update_in_session", request);
        match lock(&self.update_error).clone() {
            Some(error) => Err(error),
            None => Ok(self.outcome(request.agent_instance_id, &request.plan_hash, false)),
        }
    }
    async fn verify(&self, id: AgentInstanceId) -> Result<VerifyOutcome, InstallError> {
        lock(&self.log).push("installers.verify".to_owned());
        if let Some(outcome) = lock(&self.verify_for).get(&id) {
            return Ok(outcome.clone());
        }
        lock(&self.verify).clone()
    }
    async fn verify_and_repair(
        &self,
        id: AgentInstanceId,
        _: &SourceScope,
    ) -> Result<InstallOutcome, InstallError> {
        lock(&self.log).push("installers.verify_and_repair".to_owned());
        let repaired = lock(&self.repair).clone()?;
        Ok(self.outcome(id, "", repaired))
    }
    async fn uninstall(&self, _: AgentInstanceId) -> Result<(), InstallError> {
        lock(&self.log).push("installers.uninstall".to_owned());
        match lock(&self.uninstall_error).clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    async fn record(&self, id: AgentInstanceId) -> Result<InstallRecord, InstallError> {
        if let Some(error) = lock(&self.record_error).clone() {
            return Err(error);
        }
        Ok(Self::record_of(id, ""))
    }
}

// ---- the Agent Folder Manager ----

pub struct FakeFolders {
    pub create_error: Mutex<Option<AfmError>>,
    pub remove_error: Mutex<Option<AfmError>>,
    pub log: Log,
}

impl FakeFolders {
    pub fn new(log: Log) -> Arc<FakeFolders> {
        Arc::new(FakeFolders {
            create_error: Mutex::new(None),
            remove_error: Mutex::new(None),
            log,
        })
    }
}

#[async_trait]
impl FoldersPort for FakeFolders {
    async fn create(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError> {
        lock(&self.log).push("folders.create".to_owned());
        if let Some(error) = lock(&self.create_error).clone() {
            return Err(error);
        }
        Ok(AgentFolder {
            agent_instance_id: id,
            folder_path: PathBuf::from(format!("{ROOT}data/agents/{id}/folder")),
            meta_path: PathBuf::from(format!("{ROOT}data/agents/{id}/meta")),
            state: ordin_agent_folders::AgentFolderState::Created,
            created_at: Timestamp::now(),
        })
    }
    async fn remove(&self, _: AgentInstanceId) -> Result<(), AfmError> {
        lock(&self.log).push("folders.remove".to_owned());
        match lock(&self.remove_error).clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

// ---- the Package Inspector ----

pub struct FakeInspector {
    pub may_do: Mutex<MayDo>,
    pub candidate_may_do: Mutex<MayDo>,
    pub inspect_error: Mutex<Option<InspectError>>,
    pub candidate_error: Mutex<Option<InspectError>>,
    pub forget_error: Mutex<Option<InspectError>>,
    pub workspaces: Mutex<Vec<Vec<WorkspaceId>>>,
    pub workspaces_error: Mutex<Option<InspectError>>,
    pub requests: Mutex<Vec<InspectRequest>>,
    pub model_error: Mutex<Option<InspectError>>,
    pub log: Log,
}

pub fn may_do(connections: &[&str]) -> MayDo {
    MayDo {
        tools: vec!["Write".to_owned()],
        reads: vec!["Read".to_owned()],
        connections: connections.iter().map(|c| (*c).to_owned()).collect(),
        tool_policy_digest: Some("p1".to_owned()),
        declaration_digest: Some("d1".to_owned()),
        authority: Authority::DocumentOnly,
    }
}

fn tool(name: &str, permission: &str) -> ToolEntry {
    ToolEntry {
        name: name.to_owned(),
        kind: ToolKind::BuiltIn,
        permission: permission.to_owned(),
        declared_by: "declaration".to_owned(),
        arguments: Vec::new(),
    }
}

pub fn model_of(id: AgentInstanceId, may_do: &MayDo) -> PackageModel {
    PackageModel {
        model_schema: MODEL_SCHEMA.to_owned(),
        agent_instance_id: id,
        source: ModelSource::OrdinBundle {
            plan_hash: digest(),
            repository: REPO_LABEL.to_owned(),
            commit: COMMIT_A.to_owned(),
        },
        display_name: "Analyst".to_owned(),
        description: String::new(),
        tasks: vec![TaskEntry {
            command_id: "urn:ordin:acme:commands:plan".to_owned(),
            name: "Plan the release".to_owned(),
            description: String::new(),
            session_name: None,
            argument_hint: None,
        }],
        tools: may_do
            .tools
            .iter()
            .map(|name| tool(name, "requires-approval"))
            .chain(may_do.reads.iter().map(|name| tool(name, "read-only")))
            .collect(),
        connections: may_do
            .connections
            .iter()
            .map(|name| ConnectionEntry {
                name: name.clone(),
                server_type: "stdio".to_owned(),
                transport: None,
                needs_credential: false,
            })
            .collect(),
        authority: may_do.authority,
        authority_source: AuthoritySource::Derived,
        not_carried: Vec::new(),
        declaration_sha256: may_do.declaration_digest.clone(),
        declared_rules: Vec::new(),
        tool_policy_sha256: may_do.tool_policy_digest.clone(),
        plugin_name: None,
        needs_person_present: false,
        memory: None,
        parts: Vec::new(),
        inspected_at: Timestamp::now(),
    }
}

impl FakeInspector {
    pub fn new(log: Log) -> Arc<FakeInspector> {
        Arc::new(FakeInspector {
            may_do: Mutex::new(may_do(&["files"])),
            candidate_may_do: Mutex::new(may_do(&["files"])),
            inspect_error: Mutex::new(None),
            candidate_error: Mutex::new(None),
            forget_error: Mutex::new(None),
            workspaces: Mutex::new(Vec::new()),
            workspaces_error: Mutex::new(None),
            requests: Mutex::new(Vec::new()),
            model_error: Mutex::new(None),
            log,
        })
    }
}

#[async_trait]
impl InspectorPort for FakeInspector {
    async fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError> {
        lock(&self.log).push("inspector.inspect".to_owned());
        lock(&self.requests).push(request.clone());
        if let Some(error) = lock(&self.inspect_error).clone() {
            return Err(error);
        }
        let may_do = lock(&self.may_do).clone();
        Ok(InspectOutcome {
            model: model_of(request.agent_instance_id, &may_do),
            parsed_declaration_ref: format!("pki:{}:{}", request.agent_instance_id, "2".repeat(64)),
            may_do,
        })
    }
    async fn inspect_candidate(
        &self,
        request: &InspectRequest,
    ) -> Result<PackageModel, InspectError> {
        lock(&self.log).push("inspector.inspect_candidate".to_owned());
        lock(&self.requests).push(request.clone());
        if let Some(error) = lock(&self.candidate_error).clone() {
            return Err(error);
        }
        let may_do = lock(&self.candidate_may_do).clone();
        Ok(model_of(request.agent_instance_id, &may_do))
    }
    async fn forget(&self, _: AgentInstanceId) -> Result<(), InspectError> {
        lock(&self.log).push("inspector.forget".to_owned());
        match lock(&self.forget_error).clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    async fn set_workspaces(
        &self,
        _: AgentInstanceId,
        workspace_ids: Vec<WorkspaceId>,
    ) -> Result<(), InspectError> {
        lock(&self.log).push("inspector.set_workspaces".to_owned());
        if let Some(error) = lock(&self.workspaces_error).clone() {
            return Err(error);
        }
        lock(&self.workspaces).push(workspace_ids);
        Ok(())
    }
    async fn model(&self, id: AgentInstanceId) -> Result<PackageModel, InspectError> {
        if let Some(error) = lock(&self.model_error).clone() {
            return Err(error);
        }
        Ok(model_of(id, &lock(&self.may_do).clone()))
    }
}

// ---- the program's two traits ----

pub struct FakeActivity {
    pub running: Mutex<HashSet<AgentInstanceId>>,
    pub tx: broadcast::Sender<AgentInstanceId>,
}

impl FakeActivity {
    pub fn new() -> Arc<FakeActivity> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(FakeActivity {
            running: Mutex::new(HashSet::new()),
            tx,
        })
    }

    pub fn start(&self, id: AgentInstanceId) {
        lock(&self.running).insert(id);
    }

    pub fn finish(&self, id: AgentInstanceId) {
        lock(&self.running).remove(&id);
        let _ = self.tx.send(id);
    }
}

impl InstanceActivity for FakeActivity {
    fn running(&self, id: AgentInstanceId) -> bool {
        lock(&self.running).contains(&id)
    }
    fn idle(&self) -> broadcast::Receiver<AgentInstanceId> {
        self.tx.subscribe()
    }
}

pub struct FakeHarness(pub HarnessKind);

impl HarnessSettings for FakeHarness {
    fn default_harness(&self) -> HarnessKind {
        self.0
    }
}

// ---- the fixture ----

pub struct Fx {
    pub pc: PackageCatalogue,
    pub dir: TempDir,
    pub pool: sqlx::SqlitePool,
    pub journal: Arc<Journal>,
    pub catalogue: Arc<FakeCatalogue>,
    pub resolver: Arc<FakeResolver>,
    pub checker: Arc<FakeChecker>,
    pub installers: Arc<FakeInstallers>,
    pub folders: Arc<FakeFolders>,
    pub inspector: Arc<FakeInspector>,
    pub activity: Arc<FakeActivity>,
    pub log: Log,
    pub repo: AgentRepositoryId,
    pub agent: PartId,
    pub workspace: WorkspaceId,
}

impl Fx {
    pub async fn new() -> Fx {
        let dir = TempDir::new();
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.0.join("data").join("journal.db"),
            backup_dir: dir.0.join("data").join("backups"),
        })
        .await
        .expect("open the SQLite datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("register the journal");
        registry
            .register(crate::migrations())
            .expect("register the package catalogue");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
        let journal = Arc::new(
            Journal::start(
                store.clone(),
                JournalOptions {
                    automatic_pruning: false,
                    prune_interval: Duration::from_secs(24 * 3600),
                    clock: Arc::new(SystemClock),
                },
            )
            .await
            .expect("start the journal"),
        );
        let pool = store.sqlite().expect("a SQLite pool").clone();
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let repo = AgentRepositoryId::new();
        let agent = PartId::new();
        let catalogue = FakeCatalogue::new(repo, Arc::clone(&log));
        {
            let mut parts = lock(&catalogue.parts);
            parts.push(view(
                part(
                    agent,
                    repo,
                    PartKind::WorkspaceProfile,
                    AGENT_IDENTIFIER,
                    "Analyst",
                    "1.2.0",
                ),
                true,
                PartStateLabel::Current,
            ));
            for (identifier, name) in [
                (GO_PACK, "Go"),
                (PYTEST_PACK, "pytest"),
                (UNITTEST_PACK, "unittest"),
            ] {
                parts.push(view(
                    part(
                        PartId::new(),
                        repo,
                        PartKind::PracticePack,
                        identifier,
                        name,
                        "1.0.0",
                    ),
                    true,
                    PartStateLabel::Current,
                ));
            }
        }
        let resolver = FakeResolver::new(
            dir.0.clone(),
            plan("a1", COMMIT_A, "1.2.0"),
            Arc::clone(&log),
        );
        let checker = FakeChecker::new(Arc::clone(&log));
        let installers = FakeInstallers::new(Arc::clone(&log));
        let folders = FakeFolders::new(Arc::clone(&log));
        let inspector = FakeInspector::new(Arc::clone(&log));
        let activity = FakeActivity::new();
        let ports = Ports {
            catalogue: catalogue.clone(),
            resolver: resolver.clone(),
            checker: checker.clone(),
            installers: installers.clone(),
            folders: folders.clone(),
            inspector: inspector.clone(),
        };
        let pc = PackageCatalogue::from_parts(
            ports,
            Some(pool.clone()),
            Arc::clone(&journal),
            Arc::new(FakeHarness(HarnessKind::ClaudeCode)),
            activity.clone(),
        );
        Fx {
            pc,
            dir,
            pool,
            journal,
            catalogue,
            resolver,
            checker,
            installers,
            folders,
            inspector,
            activity,
            log,
            repo,
            agent,
            workspace: WorkspaceId::new(),
        }
    }

    pub fn input(&self) -> AddAgentInput {
        AddAgentInput {
            workspace_id: self.workspace,
            agent: self.agent,
            workspace_languages: vec!["go".to_owned()],
            interface_locale: "en-GB".to_owned(),
            choices: Vec::new(),
        }
    }

    /// Adds the agent and returns it, asserting the outcome is `Ready`.
    pub async fn add(&self) -> InstalledAgent {
        match self.pc.add(self.input()).await.expect("add succeeds") {
            crate::types::AddOutcome::Ready { agent } => agent,
            other => panic!("expected ready, got {other:?}"),
        }
    }

    pub fn log_lines(&self) -> Vec<String> {
        lock(&self.log).clone()
    }

    pub fn clear_log(&self) {
        lock(&self.log).clear();
    }

    pub async fn journal_kinds(&self, kind: &str) -> usize {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.to_owned()),
                },
                None,
            )
            .await
            .expect("list the journal")
            .items
            .len()
    }

    pub fn set_plan(&self, plan: ResolutionPlan) {
        *lock(&self.resolver.plan) = plan;
    }

    /// Reads one column of one instance row, for assertions on what is stored.
    pub async fn column(&self, id: AgentInstanceId, column: &str) -> Option<String> {
        use sqlx::Row;
        let sql = format!("SELECT {column} FROM agent_instances WHERE id = ?1");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id.to_string())
            .fetch_one(&self.pool)
            .await
            .expect("select the column");
        row.try_get::<Option<String>, _>(0).expect("a text column")
    }
}

impl Fx {
    /// A second library agent of the same repository, offered, under another identifier.
    pub fn second_agent(&self, identifier: &str, name: &str) -> PartId {
        let id = PartId::new();
        lock(&self.catalogue.parts).push(view(
            part(
                id,
                self.repo,
                PartKind::WorkspaceProfile,
                identifier,
                name,
                "1.0.0",
            ),
            true,
            PartStateLabel::Current,
        ));
        id
    }

    pub async fn add_agent(&self, agent: PartId) -> InstalledAgent {
        let mut input = self.input();
        input.agent = agent;
        match self.pc.add(input).await.expect("add succeeds") {
            crate::types::AddOutcome::Ready { agent } => agent,
            other => panic!("expected ready, got {other:?}"),
        }
    }

    pub fn trial_input(&self) -> crate::types::TrialInput {
        crate::types::TrialInput {
            agent_draft_id: ordin_types::AgentDraftId::new(),
            agent_repository_id: self.repo,
            worktree: PathBuf::from("/worktrees/draft"),
            agent: AGENT_IDENTIFIER.to_owned(),
            workspace_id: self.workspace,
            workspace_languages: vec!["go".to_owned()],
            interface_locale: "en-GB".to_owned(),
        }
    }

    /// The id of the only instance row, whatever its status.
    pub async fn only_instance_id(&self) -> AgentInstanceId {
        use sqlx::Row;
        let rows = sqlx::query("SELECT id FROM agent_instances")
            .fetch_all(&self.pool)
            .await
            .expect("select ids");
        assert_eq!(rows.len(), 1, "exactly one instance row");
        AgentInstanceId::parse(&rows[0].try_get::<String, _>(0).expect("an id")).expect("an id")
    }

    pub async fn instance_count(&self) -> usize {
        use sqlx::Row;
        let row = sqlx::query("SELECT COUNT(*) FROM agent_instances")
            .fetch_one(&self.pool)
            .await
            .expect("count");
        usize::try_from(row.try_get::<i64, _>(0).expect("a count")).expect("a count")
    }

    /// Polls `check` until it holds, failing after five seconds.
    pub async fn wait_until<F, Fut>(&self, mut check: F)
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        for _ in 0..500 {
            if check().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the condition did not hold within five seconds");
    }
}

pub fn count(lines: &[String], what: &str) -> usize {
    lines.iter().filter(|l| l.as_str() == what).count()
}

pub fn shutdown() -> tokio_util::sync::CancellationToken {
    tokio_util::sync::CancellationToken::new()
}
