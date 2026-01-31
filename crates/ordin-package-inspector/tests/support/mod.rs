//! Shared helpers of this crate's integration tests. They use the public surface only: a real Rules
//! Engine over a real SQLite journal (built as the Rules Engine's own integration tests build it),
//! and agent folders and bundles written to a temporary directory in the shape the Installers and the
//! Resolver and Compiler write them. Nothing here skips.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_agent_model::{
    canonical_json, content_digest, slug_from_id, BundleArtefact, BundleManifest, LoadingMode,
    LockFile, McpServerDecl, PartFiles, PartKind, PartRef, ResolvedToolPolicy, Selections,
    BUNDLE_MANIFEST_NAME, BUNDLE_SCHEMA, LOCK_FILE_NAME, LOCK_FILE_SCHEMA,
};
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_package_inspector::{InspectRequest, InstallFacts, PackageInspector};
use ordin_rules::{
    CardWording, ContentChecker, ControlLine, FactError, MessageRule, ProgramKind, Proposal,
    RulesConfig, RulesEngine, RulesHost, RunKey,
};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, RunId};

pub const APP_ROOT: &str = "/Users/asha/Library/Application Support/ordin";
pub const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-pki-it-{}", AgentInstanceId::new()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[derive(Default)]
struct Host;

#[async_trait::async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _run: &RunKey, _path: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(
        &self,
        _run: &RunKey,
        _path: &str,
        _text: &str,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _run: &RunKey, _path: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _run: &RunKey, _path: &str, _line: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _run: &RunKey, _path: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn runs_in_progress(&self, _command: &str) -> Result<u32, FactError> {
        Ok(1)
    }
    async fn paused_run_with_command(
        &self,
        _commands: &[String],
        _except: RunId,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _run_id: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(
        &self,
        _scope: Option<&str>,
        _name: &str,
    ) -> Result<serde_json::Value, FactError> {
        Ok(serde_json::Value::Null)
    }
    async fn setting_rules(&self, _scope: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _run: &RunKey, _proposal: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "The run's work".to_owned(),
        }];
        CardWording {
            question: "Allow this action?".to_owned(),
            verb: "Allow".to_owned(),
            where_line: "This computer".to_owned(),
            affects: affects.clone(),
            changes: "The action runs.".to_owned(),
            content_label: None,
            proposal: ActionProposal {
                action: "Run a tool".to_owned(),
                destination: "This computer".to_owned(),
                affects,
                consequence: "The action runs.".to_owned(),
            },
        }
    }
    fn plain_line(&self, line: &ControlLine) -> String {
        format!("{} applies.", line.name)
    }
    fn content_checker(&self) -> Option<Arc<dyn ContentChecker>> {
        None
    }
}

pub struct Fixture {
    pub dir: TempDir,
    pub engine: Arc<RulesEngine>,
    pub inspector: PackageInspector,
    pub store_dir: PathBuf,
    _journal: Arc<Journal>,
}

fn config() -> RulesConfig {
    RulesConfig {
        ordin_servers: ["ordin", "work", "code", "mail"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        ordin_server_prefixes: vec!["craft_".to_owned()],
        protected_roots: vec![PathBuf::from("/System"), PathBuf::from("/etc")],
        app_data_root: PathBuf::from(APP_ROOT),
        program: ProgramKind::Desktop,
        instance_name: "local".to_owned(),
    }
}

impl Fixture {
    pub async fn new() -> Fixture {
        let dir = TempDir::new();
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.0.join("data").join("journal.db"),
            backup_dir: dir.0.join("data").join("backups"),
        })
        .await
        .expect("open the SQLite datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(ApprovalService::migrations())
            .expect("approval");
        registry.register(RulesEngine::migrations()).expect("rules");
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
        let approvals = ApprovalService::new(
            store.clone(),
            journal.clone(),
            ApprovalConfig {
                backend: BackendId(journal.backend_uuid()),
                local_computer_name: "Asha's laptop".to_owned(),
                approvers: None,
            },
        )
        .await
        .expect("approval service");
        let profile = Arc::new(
            OrganisationProfileService::start(
                ProfileMode::NotUsed,
                ProfileOptions {
                    refresh_interval: Duration::from_secs(3600),
                    clock: Arc::new(WallClock),
                },
            )
            .await
            .expect("profile service"),
        );
        let engine = RulesEngine::new(
            store,
            journal.clone(),
            profile,
            approvals,
            Arc::new(Host),
            config(),
        );
        let store_dir = dir.path("inspector");
        let inspector =
            PackageInspector::new(engine.clone(), store_dir.clone()).expect("the inspector");
        Fixture {
            dir,
            engine,
            inspector,
            store_dir,
            _journal: journal,
        }
    }

    /// A second inspector over the same store and engine (a program restart).
    pub fn restarted(&self) -> PackageInspector {
        PackageInspector::new(self.engine.clone(), self.store_dir.clone()).expect("the inspector")
    }

    pub fn model_file(&self, id: AgentInstanceId) -> PathBuf {
        self.store_dir.join(format!("{id}.json"))
    }

    pub fn rules_file(&self, id: AgentInstanceId) -> PathBuf {
        self.store_dir.join(format!("{id}.rules.json"))
    }
}

// ---- agent folders and bundles

/// One part to write into a bundle.
#[derive(Clone)]
pub struct PartDef {
    pub kind: PartKind,
    pub id: String,
    pub repository: String,
    pub files: Vec<(String, String)>,
}

pub fn kind_word(kind: PartKind) -> &'static str {
    match kind {
        PartKind::SystemRole => "system-role",
        PartKind::Rule => "rule",
        PartKind::Skill => "skill",
        PartKind::Command => "command",
        PartKind::ToolBinding => "tool-binding",
        PartKind::Capability => "capability",
        PartKind::WorkspaceProfile => "workspace-profile",
        PartKind::Journey => "journey",
        PartKind::Memory => "memory",
        PartKind::AgentIdentity => "agent-identity",
        _ => "rule",
    }
}

/// A Markdown part file: front matter, then a body.
pub fn markdown(kind: PartKind, id: &str, name: &str, description: &str, extra: &str) -> String {
    format!(
        "---\napiVersion: ai.enterprise/v1\nkind: {}\nmetadata:\n  id: {id}\n  name: {name}\n  version: 1.0.0\n  description: {description}\n{extra}---\nBody of {name}.\n",
        kind_word(kind)
    )
}

/// A YAML part file: the manifest keys, then `body` (top-level keys).
pub fn yaml(kind: PartKind, id: &str, name: &str, description: &str, body: &str) -> String {
    format!(
        "apiVersion: ai.enterprise/v1\nkind: {}\nmetadata:\n  id: {id}\n  name: {name}\n  version: 1.0.0\n  description: {description}\n{body}",
        kind_word(kind)
    )
}

pub fn urn(dir: &str, slug: &str) -> String {
    format!("urn:ordin:default:{dir}:{slug}")
}

impl PartDef {
    pub fn markdown_part(
        kind: PartKind,
        dir: &str,
        slug: &str,
        name: &str,
        description: &str,
    ) -> PartDef {
        let id = urn(dir, slug);
        let file = markdown(kind, &id, name, description, "");
        PartDef {
            kind,
            id,
            repository: "bundled".to_owned(),
            files: vec![(format!("{slug}.md"), file)],
        }
    }

    pub fn command(slug: &str, name: &str, hint: Option<&str>) -> PartDef {
        let id = urn("commands", slug);
        let extra = hint
            .map(|h| format!("argument-hint: \"{h}\"\n"))
            .unwrap_or_default();
        let file = markdown(
            PartKind::Command,
            &id,
            name,
            &format!("{name} one phase of a plan. Further words."),
            &extra,
        );
        PartDef {
            kind: PartKind::Command,
            id,
            repository: "bundled".to_owned(),
            files: vec![(format!("{slug}.md"), file)],
        }
    }

    pub fn yaml_part(kind: PartKind, dir: &str, slug: &str, name: &str, body: &str) -> PartDef {
        let id = urn(dir, slug);
        let file = yaml(kind, &id, name, &format!("{name} part."), body);
        PartDef {
            kind,
            id,
            repository: "bundled".to_owned(),
            files: vec![(format!("{slug}.yaml"), file)],
        }
    }

    pub fn tool_binding(slug: &str, body: &str) -> PartDef {
        PartDef::yaml_part(PartKind::ToolBinding, "tool-bindings", slug, slug, body)
    }

    pub fn in_repository(mut self, repository: &str) -> PartDef {
        self.repository = repository.to_owned();
        self
    }

    pub fn part_ref(&self) -> PartRef {
        let files = PartFiles(
            self.files
                .iter()
                .map(|(path, text)| (path.clone(), text.clone().into_bytes()))
                .collect(),
        );
        PartRef {
            kind: self.kind,
            id: self.id.clone(),
            name: slug_from_id(&self.id),
            version: "1.0.0".to_owned(),
            content_hash: format!("sha256:{}", content_digest(&files).0),
            repository: self.repository.clone(),
            commit: COMMIT.to_owned(),
            loading: Some(LoadingMode::AlwaysOn),
        }
    }
}

/// An installed ordin agent on disk.
pub struct Installed {
    pub folder: PathBuf,
    pub bundle_dir: PathBuf,
    pub lock: LockFile,
    pub bundle: BundleManifest,
}

pub fn composition() -> PartDef {
    composition_named("Software Engineer (CSPA-431)")
}

pub fn composition_named(name: &str) -> PartDef {
    let id = urn("workspaces", "software-engineer");
    let file = yaml(
        PartKind::WorkspaceProfile,
        &id,
        name,
        "The software-engineering workspace that runs the capture, structure, plan, and apply loop. It has ten commands.",
        "",
    );
    PartDef {
        kind: PartKind::WorkspaceProfile,
        id,
        repository: "bundled".to_owned(),
        files: vec![("workspace-profile.yaml".to_owned(), file)],
    }
}

pub fn no_policy() -> ResolvedToolPolicy {
    ResolvedToolPolicy::default()
}

pub fn mcp_server(name: &str, env_ref: Option<&str>) -> McpServerDecl {
    McpServerDecl {
        name: name.to_owned(),
        server_type: "stdio".to_owned(),
        scope: None,
        env_ref: env_ref.map(str::to_owned),
        transport: Some("stdio".to_owned()),
        declared_by: urn("tool-bindings", name),
    }
}

/// Writes `<root>/folder/ordin.lock` and `<root>/bundles/<hex>/...` for `parts`.
pub fn install(
    root: &Path,
    label: &str,
    parts: &[PartDef],
    policy: ResolvedToolPolicy,
) -> Installed {
    install_named(root, label, "Software Engineer (CSPA-431)", parts, policy)
}

pub fn install_named(
    root: &Path,
    label: &str,
    name: &str,
    parts: &[PartDef],
    policy: ResolvedToolPolicy,
) -> Installed {
    let plan_hash = format!(
        "sha256:{}",
        content_digest(&PartFiles(
            [(label.to_owned(), label.as_bytes().to_vec())]
                .into_iter()
                .collect()
        ))
        .0
    );
    let composition = composition_named(name);
    let composition_ref = composition.part_ref();
    let mut sorted: Vec<PartRef> = parts.iter().map(PartDef::part_ref).collect();
    sorted.sort();
    let lock = LockFile {
        lockfile_schema: LOCK_FILE_SCHEMA.to_owned(),
        plan_hash: plan_hash.clone(),
        target_harness: "claude_code".to_owned(),
        composition: composition_ref.clone(),
        artefacts: sorted.clone(),
        overlays_applied: Vec::new(),
        selections: Selections::default(),
    };
    let bundle = BundleManifest {
        bundle_schema: BUNDLE_SCHEMA.to_owned(),
        plan_hash: plan_hash.clone(),
        composition: composition_ref,
        artefacts: sorted
            .iter()
            .map(|part| BundleArtefact {
                part: part.clone(),
                loading: LoadingMode::AlwaysOn,
            })
            .collect(),
        overlays_applied: Vec::new(),
        resolved_tool_policy: policy,
        lockfile_sha256: "0".repeat(64),
        compiler_version: "0.1.0".to_owned(),
    };
    let folder = root.join(label).join("folder");
    let bundle_dir = root
        .join(label)
        .join("bundles")
        .join(plan_hash.trim_start_matches("sha256:"));
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::create_dir_all(&bundle_dir).expect("bundle");
    std::fs::write(folder.join(LOCK_FILE_NAME), lock.to_canonical_yaml()).expect("lock");
    std::fs::write(
        bundle_dir.join(BUNDLE_MANIFEST_NAME),
        canonical_json(&serde_json::to_value(&bundle).expect("bundle json")),
    )
    .expect("bundle manifest");
    for def in parts.iter().chain(std::iter::once(&composition)) {
        let dir = bundle_dir
            .join("artefacts")
            .join(def.part_ref().content_hash.trim_start_matches("sha256:"));
        for (path, text) in &def.files {
            let target = dir.join(path);
            std::fs::create_dir_all(target.parent().expect("parent")).expect("part dir");
            std::fs::write(target, text).expect("part file");
        }
    }
    Installed {
        folder,
        bundle_dir,
        lock,
        bundle,
    }
}

impl Installed {
    pub fn request(&self, id: AgentInstanceId) -> InspectRequest {
        InspectRequest {
            agent_instance_id: id,
            folder_path: self.folder.clone(),
            bundle_dir: Some(self.bundle_dir.clone()),
            repository_names: BTreeMap::new(),
            part_states: BTreeMap::new(),
            install: Some(InstallFacts {
                plugin_name: Some("cspa-431".to_owned()),
                command_names: BTreeMap::new(),
                reference_only_parts: Vec::new(),
                needs_person_present: true,
            }),
        }
    }

    pub fn declaration(&self, text: &str) {
        std::fs::write(self.folder.join("wrightrun-declaration.yaml"), text).expect("declaration");
    }
}

/// Ten commands, as the happy-path scenario installs.
pub fn ten_commands() -> Vec<PartDef> {
    [
        "capture",
        "structure",
        "plan",
        "apply",
        "review",
        "sync",
        "rca",
        "rfc",
        "spec",
        "design",
    ]
    .iter()
    .map(|slug| PartDef::command(slug, &slug.to_uppercase(), None))
    .collect()
}

/// A request for an APM-era folder (no lock file, no bundle).
pub fn apm_request(id: AgentInstanceId, folder: &Path) -> InspectRequest {
    InspectRequest {
        agent_instance_id: id,
        folder_path: folder.to_path_buf(),
        bundle_dir: None,
        repository_names: BTreeMap::new(),
        part_states: BTreeMap::new(),
        install: None,
    }
}

/// True when the Rules Engine holds a rule set for `id` (a run of it can begin).
pub async fn loaded(engine: &RulesEngine, id: AgentInstanceId) -> bool {
    let facts = ordin_rules::RunStartFacts {
        run_id: RunId::new(),
        agent_instance_id: id,
        workspace_id: ordin_types::WorkspaceId::new(),
        crew_charter_id: None,
        command: None,
        text: String::new(),
        scope: None,
        asker: None,
        started_by: "person".to_owned(),
        started_by_id: "person:asha".to_owned(),
    };
    !matches!(
        engine.begin_run(facts).await,
        Err(ordin_rules::RulesError::RuleSetNotLoaded)
    )
}

/// A declaration the Rules Engine refuses: a step gate that names a tool nothing binds.
pub const UNBOUND_DECLARATION: &str = "format-version: 1\ngates:\n  steps:\n    - name: ghost-gate\n      point: before-tool\n      tool: ghost\n      conditions: []\n      outcome:\n        kind: refuse\n";

pub fn fixed_id() -> AgentInstanceId {
    AgentInstanceId::parse("0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff").expect("a UUIDv7")
}
