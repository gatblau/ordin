//! `RulesEngine`: construction, loading rule sets, grants, the calls outside the three decision
//! points that need no store, and the shared helpers of the decision modules (RulesEngine.md
//! *Public Interface*, *Internal Logic* A, F, G, K.4–K.9).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use ordin_approval::ApprovalService;
use ordin_journal::{Datastore, Journal, MigrationStep, ModuleMigrations, NewJournalEntry};
use ordin_organisation_profile::{OrganisationProfileService, ProfileSnapshot};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::places::{FolderAccess, Grantee};
use ordin_types::time::Timestamp;
use ordin_types::wire::rules::{RuleRef, RuleSource, ToolDecision, Verdict};
use ordin_types::wire::startup::{NotEnforcedReason, StartUpReportItem};
use ordin_types::{AgentInstanceId, CrewCharterId, PlaceId, RunId, WorkspaceId};
use sqlx::SqlitePool;
use tokio::task::JoinHandle;

use crate::error::{codes, RulesError};
use crate::facts::RunFacts;
use crate::load::{self, ControlSet};
use crate::policy;
use crate::store;
use crate::types::{
    ControlLine, EmailCheck, EmailVerdict, GitHostSetting, MetricsSink, PathAccessRequest,
    PlaceRead, PlaceReader, ProgramKind, RulesConfig, RulesHost, RunRestriction,
};
use crate::vocab::{GrantedPlace, InstanceRuleSet, MessageRule};
use crate::{LEASE_RENEW_SECONDS, LEASE_SECONDS, RUN_FACTS_TTL_SECONDS};

const SQLITE_MIGRATIONS: &[MigrationStep] =
    &[(1, "create rules tables", include_str!("sql/sqlite_v1.sql"))];
const POSTGRES_MIGRATIONS: &[MigrationStep] = &[(
    1,
    "create rules tables",
    include_str!("sql/postgres_v1.sql"),
)];

/// Rows of runs that never ended cleanly are deleted at start after this many days (*Data Model*).
const STALE_DAYS: i64 = 30;
/// The longest `PlaceRead.path` (K.4).
const MAX_READ_PATH: usize = 4_096;
/// `Deny.reason` 1–500 characters (2C).
const MAX_REASON_CHARS: usize = 500;

pub(crate) const FOLDER_REASON: &str = "the path is outside the places this agent may use";
pub(crate) const NOT_LOADED_REASON: &str = "This agent's rules are not loaded.";

/// Where the engine reads the organisation profile: the service in the programs, a fixed
/// snapshot in this crate's tests.
pub(crate) trait ProfileSource: Send + Sync {
    fn current(&self) -> Arc<ProfileSnapshot>;
}

impl ProfileSource for OrganisationProfileService {
    fn current(&self) -> Arc<ProfileSnapshot> {
        OrganisationProfileService::current(self)
    }
}

/// A poisoned lock still holds consistent data here (every critical section is a plain change).
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn now_ms() -> i64 {
    Timestamp::now().unix_millis()
}

/// A deny reason within 1–500 characters.
pub(crate) fn reason(text: &str) -> String {
    let cut: String = text.chars().take(MAX_REASON_CHARS).collect();
    if cut.is_empty() {
        "refused".to_owned()
    } else {
        cut
    }
}

/// One decision before it is journalled.
#[derive(Debug, Clone)]
pub(crate) struct Decided {
    pub verdict: Verdict,
    pub source: RuleSource,
    pub rule_id: String,
    pub approval_id: Option<ordin_types::ApprovalId>,
    /// The registry code of a fail-closed or coded denial, written to the log.
    pub code: Option<&'static str>,
}

impl Decided {
    pub(crate) fn deny(
        source: RuleSource,
        rule_id: &str,
        why: &str,
        code: Option<&'static str>,
    ) -> Decided {
        Decided {
            verdict: Verdict::Deny {
                reason: reason(why),
            },
            source,
            rule_id: rule_id.chars().take(128).collect(),
            approval_id: None,
            code,
        }
    }

    pub(crate) fn allow(source: RuleSource, rule_id: &str) -> Decided {
        Decided {
            verdict: Verdict::Allow,
            source,
            rule_id: rule_id.to_owned(),
            approval_id: None,
            code: None,
        }
    }

    pub(crate) fn decision(&self, agent: Option<AgentInstanceId>) -> ToolDecision {
        let agent = match self.source {
            RuleSource::DeclarationGate
            | RuleSource::DeclarationToolRule
            | RuleSource::DeclarationArgumentRule
            | RuleSource::AgentToolPolicy => agent,
            _ => None,
        };
        ToolDecision {
            verdict: self.verdict.clone(),
            rule: RuleRef {
                source: self.source,
                rule_id: self.rule_id.clone(),
                agent_instance_id: agent,
            },
            decided_at: Timestamp::now(),
        }
    }

    pub(crate) fn kind(&self) -> &'static str {
        match self.verdict {
            Verdict::Allow => "allow",
            Verdict::Deny { .. } => "deny",
            Verdict::Ask { .. } => "ask",
        }
    }
}

pub(crate) fn source_text(source: RuleSource) -> &'static str {
    match source {
        RuleSource::DeclarationGate => "declaration_gate",
        RuleSource::DeclarationToolRule => "declaration_tool_rule",
        RuleSource::DeclarationArgumentRule => "declaration_argument_rule",
        RuleSource::AgentToolPolicy => "agent_tool_policy",
        RuleSource::AppFolderPolicy => "app_folder_policy",
        RuleSource::AppShellPolicy => "app_shell_policy",
        RuleSource::AppApprovalPolicy => "app_approval_policy",
        RuleSource::FailClosed => "fail_closed",
    }
}

/// The Rules Engine. Consumers hold `Arc<ordin_rules::RulesEngine>` (SA-RECON-01).
// spec: ordin-desktop/components/RulesEngine#SA-RECON-01
pub struct RulesEngine {
    pub(crate) datastore: Datastore,
    pub(crate) journal: Arc<Journal>,
    pub(crate) profile: Arc<dyn ProfileSource>,
    pub(crate) approvals: Arc<ApprovalService>,
    pub(crate) host: Arc<dyn RulesHost>,
    pub(crate) config: RulesConfig,
    pub(crate) sets: RwLock<HashMap<AgentInstanceId, Arc<ControlSet>>>,
    pub(crate) grants: RwLock<HashMap<WorkspaceId, Vec<GrantedPlace>>>,
    pub(crate) git_settings: RwLock<HashMap<PlaceId, Vec<GitHostSetting>>>,
    pub(crate) runs: Mutex<HashMap<RunId, (RunFacts, Instant)>>,
    pub(crate) restrictions: Mutex<HashMap<RunId, RunRestriction>>,
    pub(crate) renewals: Mutex<HashMap<RunId, JoinHandle<()>>>,
}

impl Drop for RulesEngine {
    fn drop(&mut self) {
        for (_, task) in lock(&self.renewals).drain() {
            task.abort();
        }
    }
}

impl RulesEngine {
    pub const MODULE: &'static str = "rules";

    /// Module `rules` (DatabaseMigrations.md *Modules that register migrations*).
    // spec: ordin-desktop/components/RulesEngine#SA-RECON-09
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: Self::MODULE,
            sqlite: SQLITE_MIGRATIONS,
            postgres: POSTGRES_MIGRATIONS,
        }
    }

    /// Called by: the desktop and server programs at start.
    pub fn new(
        datastore: Datastore,
        journal: Arc<Journal>,
        profile: Arc<OrganisationProfileService>,
        approvals: Arc<ApprovalService>,
        host: Arc<dyn RulesHost>,
        config: RulesConfig,
    ) -> Arc<Self> {
        Self::build(datastore, journal, profile, approvals, host, config)
    }

    pub(crate) fn build(
        datastore: Datastore,
        journal: Arc<Journal>,
        profile: Arc<dyn ProfileSource>,
        approvals: Arc<ApprovalService>,
        host: Arc<dyn RulesHost>,
        config: RulesConfig,
    ) -> Arc<Self> {
        let engine = Arc::new(RulesEngine {
            datastore,
            journal,
            profile,
            approvals,
            host,
            config,
            sets: RwLock::new(HashMap::new()),
            grants: RwLock::new(HashMap::new()),
            git_settings: RwLock::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            restrictions: Mutex::new(HashMap::new()),
            renewals: Mutex::new(HashMap::new()),
        });
        // Rows of runs that never ended cleanly are cleared at start; `new` is not async, so the
        // statement runs on the program's runtime and a failure is only logged.
        if let (Ok(runtime), Some(pool)) = (
            tokio::runtime::Handle::try_current(),
            engine.datastore.sqlite().cloned(),
        ) {
            runtime.spawn(async move {
                let before = now_ms() - STALE_DAYS * 24 * 3_600 * 1_000;
                if let Err(error) = store::delete_stale(&pool, before).await {
                    tracing::warn!(code = error.code(), "rules_stale_rows_kept");
                }
            });
        }
        engine
    }

    /// The SQLite pool of the control-state tables.
    pub(crate) fn pool(&self) -> Result<&SqlitePool, RulesError> {
        // Release 1 builds the desktop profile only: the migrations of both engines are registered, and a PostgreSQL datastore answers every statement with
        // `RULE_STATE_UNAVAILABLE`, so decisions fail closed until the scaled shape is built.
        self.datastore.sqlite().ok_or_else(|| {
            RulesError::State(sqlx::Error::Configuration(
                "the rules store is built for SQLite only".into(),
            ))
        })
    }

    /// The profile in force; the agent profile reads none (2D Gaps item 1).
    pub(crate) fn snapshot(&self) -> Arc<ProfileSnapshot> {
        if self.config.program.is_agent() {
            Arc::new(ProfileSnapshot::defaults())
        } else {
            self.profile.current()
        }
    }

    pub(crate) fn metrics(&self) -> Option<Arc<dyn MetricsSink>> {
        self.host.metrics_sink()
    }

    pub(crate) fn set_of(&self, id: AgentInstanceId) -> Option<Arc<ControlSet>> {
        read(&self.sets).get(&id).cloned()
    }

    // ---- A. rule sets

    /// Returns the start-up lines. A load error leaves the previous set in place.
    pub fn load_rule_set(&self, set: InstanceRuleSet) -> Result<Vec<ControlLine>, RulesError> {
        let id = set.agent_instance_id;
        match load::compile_set(&set, self.config.program) {
            Ok(compiled) => {
                let lines = compiled.lines.iter().map(|(l, _)| l.clone()).collect();
                write(&self.sets).insert(id, Arc::new(compiled));
                Ok(lines)
            }
            Err(error) => {
                tracing::warn!(
                    agent_instance_id = id.to_string(),
                    code = error.code(),
                    "rules_load_failed"
                );
                Err(error)
            }
        }
    }

    /// Whether the agent's package declares an ordin server, by its name or by one of its tools.
    /// False when no rule set is loaded.
    // spec: ordin-desktop/components/RulesEngine#SA-RECON-13
    pub fn declares_tool(&self, agent: AgentInstanceId, server: &str, tool: &str) -> bool {
        self.set_of(agent).is_some_and(|set| {
            set.declared_servers.iter().any(|s| s == server) || set.is_bound(tool)
        })
    }

    /// Whether the agent may run commands (`system_changing`). False when no rule set is loaded.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-11
    pub fn may_run_commands(&self, agent: AgentInstanceId) -> bool {
        self.set_of(agent)
            .is_some_and(|set| Self::is_system_changing(&set))
    }

    pub fn unload_rule_set(&self, agent_instance_id: AgentInstanceId) {
        write(&self.sets).remove(&agent_instance_id);
    }

    /// The one path from the Workspace Service's grants to enforcement.
    pub fn replace_grants(&self, workspace_id: WorkspaceId, grants: Vec<GrantedPlace>) {
        write(&self.grants).insert(workspace_id, grants);
    }

    pub fn replace_git_host_settings(&self, project_place: PlaceId, settings: Vec<GitHostSetting>) {
        write(&self.git_settings).insert(project_place, settings);
    }

    pub fn restrict_run(&self, run_id: RunId, restriction: RunRestriction) {
        lock(&self.restrictions).insert(run_id, restriction);
        if let Some((facts, _)) = lock(&self.runs).get_mut(&run_id) {
            facts.restriction = Some(restriction);
        }
    }

    pub(crate) fn restriction_of(&self, facts: &RunFacts) -> Option<RunRestriction> {
        lock(&self.restrictions)
            .get(&facts.run_id)
            .copied()
            .or(facts.restriction)
    }

    // ---- F. start-up items

    pub fn startup_items(&self, agent_instance_id: AgentInstanceId) -> Vec<StartUpReportItem> {
        let Some(set) = self.set_of(agent_instance_id) else {
            return Vec::new();
        };
        set.lines
            .iter()
            .map(|(line, source)| {
                let rule = RuleRef {
                    source: *source,
                    rule_id: line.name.clone(),
                    agent_instance_id: Some(agent_instance_id),
                };
                let plain = self.host.plain_line(line);
                if line.enforced {
                    StartUpReportItem::EnforcedRule { rule, plain }
                } else {
                    StartUpReportItem::RuleNotEnforced {
                        rule,
                        plain,
                        reason: NotEnforcedReason::RouteCannotCarry,
                    }
                }
            })
            .collect()
    }

    // ---- H.2 / K.5 / K.6 paths

    pub(crate) fn protected(&self, path: &Path) -> bool {
        self.config
            .protected_roots
            .iter()
            .any(|root| policy::is_under(path, root))
    }

    /// H.2 for one instance: `path` is absolute and normalised.
    pub(crate) fn path_allowed(
        &self,
        set: &ControlSet,
        workspace: Option<WorkspaceId>,
        crew: Option<CrewCharterId>,
        path: &Path,
        writes: bool,
    ) -> bool {
        // Every check holds for the path as written and for the file it opens, so neither a link
        // nor a name the file system folds reaches what the written path does not show.
        // spec: ordin-desktop/components/RulesEngine#SA-RECON-14
        let Some(real) = policy::real_path(path) else {
            return false;
        };
        let folder = policy::real_path(&set.folder).unwrap_or_else(|| set.folder.clone());
        let granted = |access_needed: bool| {
            let Some(workspace) = workspace else {
                return false;
            };
            read(&self.grants).get(&workspace).is_some_and(|grants| {
                grants.iter().any(|grant| {
                    let mine = match grant.grantee {
                        Grantee::AgentInstance { agent_instance_id } => agent_instance_id == set.id,
                        Grantee::Crew { crew_charter_id } => Some(crew_charter_id) == crew,
                    };
                    let root = policy::real_path(&grant.root);
                    mine && policy::is_under(path, &grant.root)
                        && root.is_some_and(|root| policy::is_under(&real, &root))
                        && (!access_needed || grant.access == FolderAccess::ReadAndChange)
                })
            })
        };
        let in_folder = policy::is_under(path, &set.folder) && policy::is_under(&real, &folder);
        let refused = |path: &Path, folder: &Path| {
            // spec: ordin-desktop/components/RulesEngine#SA-RECON-11
            (writes && (policy::in_git_folder(path) || policy::is_installed_file(path, folder)))
                || (policy::is_under(path, &self.config.app_data_root)
                    && !policy::is_under(path, folder))
        };
        // `/tmp` and `/var` resolve into `/private` on macOS, so inside the agent's own folder or a
        // grant the resolved form of a protected root is not a refusal: K.6 kept the root out.
        let protected_real = self.protected(&real) && !in_folder && !granted(false);
        if self.protected(path) || protected_real {
            return false;
        }
        if refused(path, &set.folder) || refused(&real, &folder) {
            return false;
        }
        if in_folder && (!writes || !set.read_only) {
            return true;
        }
        granted(writes)
    }

    /// K.6.
    pub fn is_protected_location(&self, path: &Path) -> bool {
        let Some(path) = policy::absolute(None, path) else {
            return true;
        };
        policy::is_fs_root(&path)
            || self.protected(&path)
            || policy::is_under(&path, &self.config.app_data_root)
    }

    /// K.5. Allow or Deny only; never Ask; not journalled.
    pub fn check_path_access(&self, req: &PathAccessRequest) -> ToolDecision {
        let folder_deny = || {
            Decided::deny(
                RuleSource::AppFolderPolicy,
                "app.folder_grant",
                FOLDER_REASON,
                Some(codes::PATH_NOT_ALLOWED),
            )
        };
        let decided = match policy::absolute(None, &req.path) {
            None => folder_deny(),
            Some(path) => match req.agent {
                None => {
                    let ordin_own = policy::is_under(&path, &self.config.app_data_root);
                    if self.protected(&path) && !ordin_own {
                        folder_deny()
                    } else {
                        Decided::allow(RuleSource::AppFolderPolicy, "app.folder_grant")
                    }
                }
                Some(id) => match self.set_of(id) {
                    None => Decided::deny(
                        RuleSource::FailClosed,
                        "app.ruleset",
                        NOT_LOADED_REASON,
                        Some(codes::RULESET_NOT_LOADED),
                    ),
                    Some(set) => {
                        let writes = req.access == FolderAccess::ReadAndChange;
                        if self.path_allowed(
                            &set,
                            req.workspace_id,
                            req.crew_charter_id,
                            &path,
                            writes,
                        ) {
                            Decided::allow(RuleSource::AppFolderPolicy, "app.folder_grant")
                        } else {
                            folder_deny()
                        }
                    }
                },
            },
        };
        match req.agent {
            Some(agent) => tracing::debug!(
                agent = agent.to_string(),
                decision = decided.kind(),
                "rules_path_checked"
            ),
            None => tracing::debug!(decision = decided.kind(), "rules_path_checked"),
        }
        decided.decision(req.agent)
    }

    /// K.4. Never fails: in-memory grants only.
    pub fn may_read(&self, check: &PlaceRead) -> bool {
        let allowed = self.may_read_inner(check);
        if !allowed {
            let reader_kind = match check.reader {
                PlaceReader::Agent { .. } => "agent",
                PlaceReader::CrewMember { .. } => "crew_member",
                PlaceReader::Ordin => "ordin",
                PlaceReader::HostedAgent => "hosted_agent",
            };
            tracing::debug!(
                workspace_id = check.workspace_id.to_string(),
                place_id = check.place_id.to_string(),
                reader_kind,
                "rules_read_refused"
            );
        }
        allowed
    }

    fn may_read_inner(&self, check: &PlaceRead) -> bool {
        let path = &check.path;
        if path.is_empty()
            || path.chars().count() > MAX_READ_PATH
            || path.contains('\0')
            || path.starts_with('/')
            || Path::new(path).is_absolute()
        {
            return false;
        }
        // `..` segments after lexical normalisation: any that climbs out of the place.
        let mut depth: i64 = 0;
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    depth -= 1;
                    if depth < 0 {
                        return false;
                    }
                }
                _ => depth += 1,
            }
        }
        let grants = read(&self.grants);
        let Some(grants) = grants.get(&check.workspace_id) else {
            return false;
        };
        let matching: Vec<&GrantedPlace> = grants
            .iter()
            .filter(|g| g.place_id == check.place_id)
            .filter(|g| match check.reader {
                PlaceReader::HostedAgent => false,
                PlaceReader::Ordin => true,
                PlaceReader::Agent { agent_instance_id } => {
                    g.grantee == Grantee::AgentInstance { agent_instance_id }
                }
                PlaceReader::CrewMember {
                    crew_charter_id,
                    agent_instance_id,
                } => {
                    g.grantee == Grantee::AgentInstance { agent_instance_id }
                        || g.grantee == Grantee::Crew { crew_charter_id }
                }
            })
            .collect();
        matching.iter().any(|g| {
            policy::absolute(None, &g.root.join(path)).is_some_and(|full| !self.protected(&full))
        })
    }

    /// K.7. Pure: reads the current profile snapshot only.
    pub fn check_email(&self, email: &EmailCheck<'_>) -> EmailVerdict {
        policy::check_email(&self.snapshot(), email)
    }

    /// I. Public for wrightrun parity; used inside the engine by argument rules.
    pub fn parse_address(text: &str) -> Result<crate::types::Address, RulesError> {
        crate::address::parse(text)
    }

    // ---- run facts (G.1)

    pub(crate) async fn load_run(&self, run_id: RunId) -> Result<Option<RunFacts>, RulesError> {
        let ttl = Duration::from_secs(RUN_FACTS_TTL_SECONDS);
        {
            let mut runs = lock(&self.runs);
            runs.retain(|_, (_, at)| at.elapsed() < ttl);
            if let Some((facts, _)) = runs.get(&run_id) {
                return Ok(Some(facts.clone()));
            }
        }
        let pool = self.pool()?;
        let Some(text) = store::load_facts(pool, &run_id.to_string()).await? else {
            return Ok(None);
        };
        let facts: RunFacts = serde_json::from_str(&text)
            .map_err(|_| RulesError::State(sqlx::Error::Decode("run facts".into())))?;
        lock(&self.runs).insert(run_id, (facts.clone(), Instant::now()));
        Ok(Some(facts))
    }

    pub(crate) async fn save_run(&self, facts: &RunFacts) -> Result<(), RulesError> {
        let pool = self.pool()?;
        let text = serde_json::to_string(facts)
            .map_err(|_| RulesError::State(sqlx::Error::Decode("run facts".into())))?;
        store::save_facts(
            pool,
            &facts.run_id.to_string(),
            &facts.agent_instance_id.to_string(),
            &text,
            now_ms(),
        )
        .await?;
        lock(&self.runs).insert(facts.run_id, (facts.clone(), Instant::now()));
        Ok(())
    }

    // ---- G.3 leases

    /// Renews the run's leases every `LEASE_RENEW_SECONDS` until they are released.
    pub(crate) fn start_renewal(&self, run_id: RunId, leases: Vec<String>) {
        if leases.is_empty() {
            return;
        }
        let Some(pool) = self.datastore.sqlite().cloned() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let holder = run_id.to_string();
        let task = runtime.spawn(async move {
            let lease_ms = i64::from(LEASE_SECONDS) * 1_000;
            loop {
                tokio::time::sleep(Duration::from_secs(u64::from(LEASE_RENEW_SECONDS))).await;
                for key in &leases {
                    let renewed = store::renew_lock(&pool, key, &holder, now_ms(), lease_ms)
                        .await
                        .unwrap_or(false);
                    if renewed {
                        continue;
                    }
                    let retaken = store::take_lock(&pool, key, &holder, now_ms(), lease_ms)
                        .await
                        .unwrap_or(false);
                    if !retaken {
                        tracing::warn!(key = key.as_str(), "rules_lease_lost");
                    }
                }
            }
        });
        if let Some(old) = lock(&self.renewals).insert(run_id, task) {
            old.abort();
        }
    }

    /// Releases every lease the run holds and stops renewing them.
    pub(crate) async fn release_leases(&self, facts: &mut RunFacts) {
        if let Some(task) = lock(&self.renewals).remove(&facts.run_id) {
            task.abort();
        }
        if facts.leases.is_empty() {
            return;
        }
        if let Ok(pool) = self.pool() {
            let holder = facts.run_id.to_string();
            for key in &facts.leases {
                if let Err(error) = store::release_lock(pool, key, &holder).await {
                    tracing::warn!(key = key.as_str(), code = error.code(), "rules_lease_kept");
                }
            }
        }
        facts.leases.clear();
    }

    // ---- K.8, K.9, G.5, G.6

    /// "duty:<duty>" lock state (G.3).
    pub async fn lock_held(&self, key: &str) -> Result<bool, RulesError> {
        store::lock_held(self.pool()?, key, now_ms()).await
    }

    pub async fn take_intake_place(
        &self,
        intake: &str,
        run_id: &str,
        limit: u32,
    ) -> Result<bool, RulesError> {
        let pool = self.pool()?;
        let slots = store::intake_slots(pool, intake, limit).await?;
        let now = now_ms();
        let stale_before = now - i64::from(crate::PLACE_NO_RUN_SECONDS) * 1_000;
        for slot in slots {
            let candidate = match &slot.run_id {
                None => true,
                Some(holder) => match self.host.run_ended(holder).await {
                    Ok(Some(ended)) => ended,
                    Ok(None) => slot.taken_at.is_some_and(|at| at < stale_before),
                    Err(_) => false,
                },
            };
            if candidate && store::take_slot(pool, intake, &slot, run_id, now).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn forget_asker(&self, connection: &str, asker: &str) -> Result<u64, RulesError> {
        store::forget_asker(self.pool()?, connection, asker).await
    }

    pub async fn count_for_asker(&self, connection: &str, asker: &str) -> Result<u64, RulesError> {
        store::count_for_asker(self.pool()?, connection, asker).await
    }

    pub async fn count_scope(&self, scope: &str) -> Result<u64, RulesError> {
        store::count_scope(self.pool()?, scope).await
    }

    pub async fn forget_scope(&self, scope: &str) -> Result<u64, RulesError> {
        store::forget_scope(self.pool()?, scope).await
    }

    pub async fn prune_before(&self, cutoff: Timestamp) -> Result<u64, RulesError> {
        store::prune_before(self.pool()?, cutoff.unix_millis()).await
    }

    /// J. A rule kept as a setting (wrightrun section D).
    pub fn check_setting_rule(
        &self,
        agent_instance_id: AgentInstanceId,
        setting: &str,
        allowed_outcomes: &[String],
        rule: &MessageRule,
    ) -> Result<(), RulesError> {
        let set = self
            .set_of(agent_instance_id)
            .ok_or(RulesError::RuleSetNotLoaded)?;
        crate::setting::check(&set, setting, allowed_outcomes, rule)
    }

    /// A journal entry of this component; a failure is logged, never returned.
    pub(crate) async fn journal_entry(
        &self,
        kind: &str,
        run_id: Option<RunId>,
        workspace_id: Option<WorkspaceId>,
        summary: &str,
        data: serde_json::Value,
    ) -> bool {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: Self::MODULE.to_owned(),
            run_id,
            crew_run_id: None,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        match self.journal.append_entry(entry).await {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(
                    kind,
                    code = ordin_types::CodedError::code(&error),
                    "rules_entry_unrecorded"
                );
                false
            }
        }
    }

    pub(crate) fn program(&self) -> ProgramKind {
        self.config.program
    }

    pub(crate) fn is_system_changing(set: &ControlSet) -> bool {
        set.authority == Authority::SystemChanging
    }
}

/// "duty:<duty>" — the lock key of a duty (G.3). Called by: Caller API.
pub fn duty_lock_key(duty: &str) -> String {
    format!("duty:{duty}")
}
