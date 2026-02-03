//! The real [`StepRunner`]: each row of *Internal Logic* step 3 constructed with its owner's
//! constructor, and the wiring done right after it (OrdinDesktop.md step 3, *Program-side
//! implementations and calls*). The order is [`crate::plan::start_steps`]'s, not this runner's.
//!
//! Built: every row and wiring step no build switch names (rows 3.1–3.49). A row or wiring step of a
//! switch stops start with `DSK_START_FAILED`, naming its crate, until its construction is written.
//
// Row 3.28's `MemoryServiceConfig.models_dir` and `model_manifest` come from `CoreInputs`, which the
// shell fills with `<data root>/models` and the manifest of the build's models input (DSK-MD-1,
// DSK-MD-2); a missing or unlisted file only makes recall answer `MEM_MODEL_UNAVAILABLE` until the
// download of `crate::memory_models` places the files and the service re-checks them.
//
// SPEC-GAP-14: "after row 3.28" forwards the File Service's `subscribe_changes` events to
// `MemoryService::source_changed(origin_ref, current_version)`, but no spec states the `origin_ref`
// of a place file. OQ-REC-3 is settled as option (a): the desktop forwards nothing for a place file
// until a canonical `origin_ref` exists, and the marker stays with that reason.
// spec: ordin-desktop/components/OrdinDesktop#OQ-REC-3
//
// SPEC-GAP-15: row 3.32's `ClaudeAdapterConfig.claude_path` is "found by the Setup Assistant"
// (`ready_harness().location`, SetupAssistant.md), which is built at row 3.43, after the adapter, and
// the config is fixed at construction. OQ-REC-4 is settled as option (a), with option (b) accepted
// until `ClaudeLocalAdapter.md` changes: the shell passes the path in `CoreInputs.claude_path`, and
// the marker stays with that reason.
// spec: ordin-desktop/components/OrdinDesktop#OQ-REC-4
//

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_brand::{Brand, BrandConfig};
use ordin_checker::Checker;
use ordin_files::{FileService, FileServiceDeps};
use ordin_hook_relay::{HookRelay, HookRelayConfig};
use ordin_identity::{
    BrowserOpener, IdentityConfig, IdentityService, Program, SecretBackendConfig,
};
use ordin_installers::Installers;
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
};
use ordin_localisation::{Localisation, LocalisationConfig, PreferenceStore};
use ordin_mcp_front::{McpFront, McpFrontConfig, ProgramProfile};
use ordin_organisation_profile::{
    HarnessName, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions,
};
use ordin_package_inspector::PackageInspector;
use ordin_resolver::{ResolverAndCompiler, ResolverConfig};
use ordin_rules::{RulesConfig, RulesEngine};
use ordin_runtime_adapter::{AdapterKind, HarnessModels};
use ordin_server_connector::{ConnectorConfig, ServerConnector, DEFAULT_READ_CACHE_BYTES};
use ordin_service_api::blocks::StandardBlockCheck;
use ordin_session_supervisor::{SessionSupervisor, SupervisorConfig};
use ordin_sharepoint::{SharePointAdapter, SharePointDeps};
use ordin_types::{BackendId, CodedError};
use ordin_usage_planner::UsagePlanner;

use ordin_agent_model::PartSource;
use ordin_catalogue::{CatalogueConfig, CatalogueDeps, CatalogueService};
use ordin_claude_adapter::{ClaudeAdapterConfig, ClaudeLocalAdapter, SignInMode};
use ordin_command_bridge::{BridgeConfig, CommandBridge, DEFAULT_SERVER_REPLAY_EVENTS};
use ordin_confluence::{ConfluenceAdapter, ConfluenceDeps};
use ordin_craft_toolkit::{CraftDeps, CraftPlatform, CraftToolDefinitions, CraftToolkit};
use ordin_crew_orchestrator::CrewOrchestrator;
use ordin_format_plugins::FormatPlugins;
use ordin_home_search::{HomeSearch, HomeSearchDeps, SearchSource};
use ordin_host::{
    CompanyVoice, CrewLimitPolicy, OrdinHost, OrdinHostConfig, OrdinHostDeps, OrdinHostPolicy,
};
use ordin_jira::{JiraAdapter, JiraDeps};
use ordin_knowledge::{KnowledgeDeps, KnowledgeService};
use ordin_memory_service::{MemoryService, MemoryServiceConfig, ModelManifest};
use ordin_package_catalogue::{BundledAgent, PackageCatalogue, PackageCatalogueDeps};
use ordin_projects::{GithubAccess, ProjectDeps, ProjectService};
use ordin_push::{PushDeps, PushService};
use ordin_runtime_adapter::RuntimeAdapter;
use ordin_server_connector::ServerEventSender;
use ordin_service_api::blocks::RefResolver;
use ordin_setup::{
    AppUpdater, BundledProgram, SetupAssistant, SetupConfig, SetupDeps, SetupProgram, UrlOpener,
};
use ordin_types::domain::HarnessKind;
use ordin_workspaces::{WorkspaceDeps, WorkspaceService};
use tokio_util::sync::CancellationToken;

use crate::core::StepRunner;
use crate::error::{StartError, START_FAILED, WRITES_NOT_ATTRIBUTED};
use crate::hosts::{DesktopRulesHost, DesktopSupervisorHost, FileAgentWrites, Late, SystemClock};
use crate::hosts_home::{
    AgentSearch, ConversationSearch, DesktopJournalReader, DesktopRefResolver, WorkflowSearch,
};
use crate::hosts_knowledge::{
    DesktopKnowledgeFiles, DesktopKnowledgeIdentity, DesktopKnowledgeMemory, DesktopKnowledgePush,
    DesktopKnowledgeRules, DesktopKnowledgeWorkspaces, NoKnowledgeFolders,
};
use crate::hosts_run::{
    confluence_settings, project_settings, BridgeStreams, DesktopCrewHost, DesktopSessionSetup,
    DesktopWorkingCopy, ProfileHarness, SetupLanguageServers, SupervisorActivity,
};
use crate::paths::DesktopPaths;
use crate::plan::{Adapter, Provider, Row, Step};
use crate::service_api::{ApiModules, DesktopErrorSentences, DesktopServiceApi};

/// The installer values the open-source build compiles in (DSK-REC-46): the feed address when the
/// build was given an `https` one, and nothing else.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-46
pub fn installer_defaults_from(feed: Option<&str>) -> InstallerSettings {
    InstallerSettings {
        update_feed_url: feed
            .filter(|url| url.starts_with("https://"))
            .map(str::to_owned),
        ..InstallerSettings::default()
    }
}

/// What the shell hands the core before step 3: the paths of step 2 and the values only the shell
/// can read (the operating system's locales, the computer's name, the bundle identifier), and the
/// shell's own implementations of the modules' ports.
pub struct CoreInputs {
    pub paths: DesktopPaths,
    /// The one outbound client (SA-HTTP-02).
    pub http: reqwest::Client,
    /// The keychain service name: the build's bundle identifier (ADR-041).
    pub bundle_identifier: String,
    /// "Asha's laptop": `ApprovalConfig.local_computer_name` and `ConnectorConfig.computer_name`.
    pub computer_name: String,
    pub installation_id: uuid::Uuid,
    /// `sys_locale::get_locales()`, most preferred first.
    pub os_locales: Vec<String>,
    /// The person's home folder, for the protected roots of RulesEngine.md H.4.
    pub home: PathBuf,
    /// The branded build's installer values (ADR-041), used when `installer.json` is absent.
    pub installer_defaults: InstallerSettings,
    /// `BrowserOpener` over `tauri-plugin-opener` (row 3.3).
    pub browser: Option<Arc<dyn BrowserOpener>>,
    /// `<data root>/models` and the digests of the build's models input (row 3.28; DSK-MD-1,
    /// DSK-MD-2).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-1
    pub models_dir: PathBuf,
    pub model_manifest: ModelManifest,
    /// The zip the build names; `None` with no models input (DSK-MD-2).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-2
    pub models_source: Option<crate::memory_models::ModelsSource>,
    /// The `claude` executable (row 3.32; SPEC-GAP-15).
    pub claude_path: PathBuf,
    /// The person's documents folder (row 3.35, `WorkspaceDeps.documents_dir`).
    pub documents_dir: PathBuf,
    /// The bundled catalogue shipped with the program (row 3.36, read-only).
    pub bundled_catalogue: PathBuf,
    /// `UrlOpener` over `tauri-plugin-opener` (row 3.43, `SetupConfig.url_opener`).
    pub url_opener: Option<Arc<dyn UrlOpener>>,
    /// `TauriUpdater` (right after row 3.43); none without the shell, as on the server.
    pub updater: Option<Arc<dyn AppUpdater>>,
    /// The window shell's dialogs, quit flow and appearance event (row 3.48); none without a
    /// window.
    pub shell: Option<Arc<dyn crate::shell::Shell>>,
}

/// One `Arc` per constructed module, in construction order.
#[derive(Default)]
pub struct Modules {
    /// Settings › Agents and models: the harness agents run with, read once at start (DSK-AGM-1).
    pub harness_in_use: Option<HarnessName>,
    pub backend: Option<BackendId>,
    pub journal: Option<Arc<Journal>>,
    pub profile: Option<Arc<OrganisationProfileService>>,
    pub identity: Option<Arc<IdentityService>>,
    pub approval: Option<Arc<ApprovalService>>,
    pub localisation: Option<Arc<Localisation>>,
    pub agent_folders: Option<Arc<AgentFolderManager>>,
    pub resolver: Option<Arc<ResolverAndCompiler>>,
    pub sharepoint: Option<Arc<SharePointAdapter>>,
    pub usage: Option<Arc<UsagePlanner>>,
    pub connector: Option<Arc<ServerConnector>>,
    pub rules: Option<Arc<RulesEngine>>,
    pub brand: Option<Arc<Brand>>,
    pub checker: Option<Arc<Checker>>,
    pub installers: Option<Arc<Installers>>,
    pub supervisor: Option<Arc<SessionSupervisor>>,
    pub mcp_front: Option<Arc<McpFront>>,
    /// Row 3.27, behind `sandbox`.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<Arc<ordin_sandbox::Sandbox>>,
    pub inspector: Option<Arc<PackageInspector>>,
    pub files: Option<Arc<FileService>>,
    pub hook_relay: Option<Arc<HookRelay>>,
    pub orchestrator: Option<Arc<CrewOrchestrator>>,
    pub memory: Option<Arc<MemoryService>>,
    /// The models' state and download, built right after row 3.28 (DSK-MD-4).
    pub memory_models: Option<crate::memory_models::MemoryModels>,
    pub craft: Option<Arc<CraftToolkit>>,
    pub confluence: Option<Arc<ConfluenceAdapter>>,
    pub jira: Option<Arc<JiraAdapter>>,
    pub projects: Option<Arc<ProjectService>>,
    pub claude: Option<Arc<ClaudeLocalAdapter>>,
    /// Row 3.33, behind `copilot`.
    #[cfg(feature = "copilot")]
    pub copilot: Option<Arc<ordin_copilot_adapter::CopilotLocalAdapter>>,
    pub workspaces: Option<Arc<WorkspaceService>>,
    pub catalogue: Option<Arc<CatalogueService>>,
    pub push: Option<Arc<PushService>>,
    pub knowledge: Option<Arc<KnowledgeService>>,
    pub packages: Option<Arc<PackageCatalogue>>,
    /// Row 3.41a, behind `team-knowledge` (DSK-TW-1).
    #[cfg(feature = "team-knowledge")]
    pub team_knowledge: Option<Arc<ordin_team_knowledge::TeamKnowledgeService>>,
    /// Rows 3.16a, 3.16b and 3.41a, behind `minutes` (DSK-MIN-1).
    #[cfg(feature = "minutes")]
    pub transcription: Option<Arc<ordin_dictation::transcription::TranscriptionService>>,
    #[cfg(feature = "minutes")]
    pub obs: Option<Arc<ordin_obs::ObsRecorder>>,
    #[cfg(feature = "minutes")]
    pub minutes: Option<Arc<ordin_minutes::MinutesService>>,
    pub ordin_host: Option<Arc<OrdinHost>>,
    /// The one `RefResolver`: `OrdinHostDeps.refs` (row 3.41) and `bind_ref_resolver`.
    pub refs: Option<Arc<DesktopRefResolver>>,
    pub home: Option<Arc<HomeSearch>>,
    /// The `JournalReader` of row 3.42, for `workspaces.tree`'s running items (DSK-REC-45).
    pub journal_reader: Option<Arc<dyn ordin_home_search::JournalReader>>,
    pub setup: Option<Arc<SetupAssistant>>,
    pub service_api: Option<Arc<DesktopServiceApi>>,
    pub sentences: Option<Arc<DesktopErrorSentences>>,
    pub bridge: Option<Arc<CommandBridge>>,
    /// The sender of row 3.49, kept for `server_connector.start` at step 4.
    pub server_events: Option<ServerEventSender>,
    /// What each step registered, in order: tool providers and runtime adapters.
    pub providers: Vec<Provider>,
    pub adapters: Vec<Adapter>,
    pub mcp_started: bool,
}

/// The real runner of step 3.
pub struct ModuleRunner {
    pub inputs: CoreInputs,
    pub modules: Modules,
    pub late: Arc<Late>,
    /// The core's shutdown token, passed to every `start(shutdown)` of step 3.
    pub shutdown: CancellationToken,
}

/// The build target's platform for the Craft Toolkit (`CraftDeps.platform`).
pub fn craft_platform() -> CraftPlatform {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        CraftPlatform::MacosArm64
    } else if cfg!(target_os = "macos") {
        CraftPlatform::MacosX86_64
    } else if cfg!(windows) {
        CraftPlatform::WindowsX86_64
    } else {
        CraftPlatform::LinuxX86_64
    }
}

/// `ProfileSnapshot::default_harness()` as a `HarnessKind` (OrganisationProfile.md, consumer table).
pub fn harness_kind(name: HarnessName) -> HarnessKind {
    match name {
        HarnessName::ClaudeCode => HarnessKind::ClaudeCode,
        HarnessName::Copilot => HarnessKind::CopilotCli,
    }
}

/// The Ordin Host's policy from the profile and the brand (row 3.41). The profile's one
/// `limits.crew` is both the company default and the company maximum.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-21
pub fn host_policy(
    document: &ordin_organisation_profile::OrganisationProfileDocument,
    coordinator_name: String,
) -> OrdinHostPolicy {
    let crew = &document.limits.crew;
    let limits = |failure_repeat_limit: u8| CrewLimitPolicy {
        rounds: u16::try_from(crew.rounds).unwrap_or(u16::MAX),
        wall_clock_minutes: u16::try_from(crew.wall_clock_minutes).unwrap_or(u16::MAX),
        members_at_once: u8::try_from(crew.members_at_once).unwrap_or(u8::MAX),
        request_budget: crew.request_budget,
        failure_repeat_limit,
    };
    let repository = document.workflows.company_repository.as_ref();
    let company_workflow_repository = repository.and_then(|repo| {
        let host = serde_json::Value::String(repo.host.clone());
        Some(ordin_host::RepositoryFolder {
            host: serde_json::from_value(host).ok()?,
            repository: repo.repository.clone(),
            folder: repo.folder.clone(),
        })
    });
    OrdinHostPolicy {
        coordinator_name,
        voice_defaults: CompanyVoice {
            tone: document.voice.tone,
            register: document.voice.register,
            terms_use: document.voice.terms_use.clone(),
            terms_avoid: document.voice.terms_avoid.clone(),
        },
        company_workflow_repository,
        crew_defaults: limits(2),
        crew_maxima: limits(5),
        repository_profile_keys: document
            .agent_catalogue
            .repositories
            .iter()
            .map(|r| r.key.clone())
            .collect(),
    }
}

/// A module's constructor failed: `StartError::ModuleFailed` with the module's own code.
fn failed<E: CodedError>(row: Row) -> impl FnOnce(E) -> StartError {
    move |error| StartError::ModuleFailed {
        module: row.module(),
        code: error.code().to_owned(),
    }
}

/// A handle an earlier row builds. Absent only if the order of step 3 were broken.
fn earlier<T: Clone>(slot: &Option<T>, row: Row) -> Result<T, StartError> {
    slot.clone().ok_or(StartError::ModuleFailed {
        module: row.module(),
        code: START_FAILED.to_owned(),
    })
}

/// DSK-WIRE-2: right after row 3.23, a Hook Relay that attributes no writes stops start.
// spec: ordin-desktop/components/OrdinDesktop#DSK-WIRE-2
pub fn check_attribution(relay: &HookRelay) -> Result<(), StartError> {
    if relay.attributes_writes() {
        Ok(())
    } else {
        Err(StartError::ModuleFailed {
            module: Row::HookRelay.module(),
            code: WRITES_NOT_ATTRIBUTED.to_owned(),
        })
    }
}

/// The MCP server names the desktop's providers claim, before the MCP Front has started (row 3.14,
/// *chosen here*): the names of RulesConfig's list whose providers this build constructs, and the
/// Project Service's four (`crates/ordin-projects/src/tools.rs`).
fn ordin_servers() -> Vec<String> {
    #[allow(unused_mut)]
    let mut names = vec![
        "ordin",
        "work",
        "memory",
        "code",
        "github",
        "gitlab",
        "checks",
        "knowledge",
    ];
    #[cfg(feature = "sandbox")]
    names.push("sandbox");
    #[cfg(feature = "mail")]
    names.push("mail");
    #[cfg(feature = "telegram")]
    names.push("telegram");
    #[cfg(feature = "slack")]
    names.push("slack");
    names.into_iter().map(str::to_owned).collect()
}

/// RulesEngine.md H.4 (SA-RULE-04): the roots never granted on this platform, then the data root.
pub fn protected_roots(home: &std::path::Path, data_root: &std::path::Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        for root in [
            "/System",
            "/Library",
            "/usr",
            "/bin",
            "/sbin",
            "/etc",
            "/private",
            "/Applications",
        ] {
            roots.push(PathBuf::from(root));
        }
        for entry in [
            "Library/Keychains",
            ".ssh",
            ".gnupg",
            ".aws",
            ".config/gh",
            ".docker",
        ] {
            roots.push(home.join(entry));
        }
    } else if cfg!(windows) {
        for root in [
            r"C:\Windows",
            r"C:\Program Files",
            r"C:\Program Files (x86)",
            r"C:\ProgramData",
        ] {
            roots.push(PathBuf::from(root));
        }
        if let Some(app_data) = std::env::var_os("APPDATA") {
            roots.push(PathBuf::from(app_data).join("Microsoft"));
        }
    } else {
        for root in [
            "/etc", "/usr", "/bin", "/sbin", "/boot", "/proc", "/sys", "/var",
        ] {
            roots.push(PathBuf::from(root));
        }
        for entry in [
            ".ssh",
            ".gnupg",
            ".aws",
            ".config/gh",
            ".docker",
            ".local/share/keyrings",
        ] {
            roots.push(home.join(entry));
        }
    }
    roots.push(data_root.to_path_buf());
    roots
}

impl ModuleRunner {
    pub fn new(inputs: CoreInputs) -> ModuleRunner {
        ModuleRunner {
            inputs,
            modules: Modules::default(),
            late: Arc::new(Late::default()),
            shutdown: CancellationToken::new(),
        }
    }

    fn data_root(&self) -> PathBuf {
        self.inputs.paths.data_root.clone()
    }

    fn journal(&self, row: Row) -> Result<Arc<Journal>, StartError> {
        earlier(&self.modules.journal, row)
    }

    fn datastore(&self, row: Row) -> Result<Datastore, StartError> {
        Ok(self.journal(row)?.datastore().clone())
    }

    /// Row 3.1.
    async fn journal_row(&mut self) -> Result<(), StartError> {
        let row = Row::Journal;
        let root = self.data_root();
        let datastore = Datastore::open(DatastoreConfig::Sqlite {
            path: root.join("journal.db"),
            backup_dir: root.join("backups"),
        })
        .await
        .map_err(failed(row))?;
        let mut registry = MigrationRegistry::new();
        crate::migrations::register_all(&mut registry).map_err(failed(row))?;
        let report = registry
            .apply(&datastore, ApplyMode::ApplyPending)
            .await
            .map_err(failed(row))?;
        let journal = Journal::start(
            datastore,
            JournalOptions {
                automatic_pruning: true,
                prune_interval: Duration::from_secs(24 * 60 * 60),
                clock: Arc::new(ordin_journal::SystemClock),
            },
        )
        .await
        .map_err(failed(row))?;
        let backend = BackendId(journal.backend_uuid());
        tracing::info!(
            component = "desktop",
            backend = backend.to_string(),
            migrated = report.applied.len(),
            "journal_open"
        );
        self.modules.backend = Some(backend);
        self.modules.journal = Some(Arc::new(journal));
        Ok(())
    }

    /// Row 3.2.
    async fn profile_row(&mut self) -> Result<(), StartError> {
        let row = Row::OrganisationProfile;
        let Some(pool) = self.datastore(row)?.sqlite().cloned() else {
            return Err(StartError::ModuleFailed {
                module: row.module(),
                code: START_FAILED.to_owned(),
            });
        };
        let profile = OrganisationProfileService::start(
            ProfileMode::Read {
                pool,
                data_root: self.data_root(),
                http: self.inputs.http.clone(),
                build_defaults: self.inputs.installer_defaults.clone(),
            },
            ProfileOptions {
                refresh_interval: Duration::from_secs(60 * 60),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .map_err(failed(row))?;
        let profile = Arc::new(profile);
        let _ = self.late.profile.set(profile.clone());
        self.modules.profile = Some(profile);
        Ok(())
    }

    /// Row 3.3.
    async fn identity_row(&mut self) -> Result<(), StartError> {
        let row = Row::Identity;
        let pool = self.datastore(row)?.sqlite().cloned();
        let identity = IdentityService::new(
            IdentityConfig {
                program: Program::Desktop,
                data_root: self.data_root(),
                keychain_service: self.inputs.bundle_identifier.clone(),
                secret_backend: SecretBackendConfig::OsKeychain,
                github_app_client_id: option_env!("GITHUB_APP_CLIENT_ID").map(str::to_owned),
                http: self.inputs.http.clone(),
                browser: self.inputs.browser.clone(),
            },
            pool,
        )
        .await
        .map_err(failed(row))?;
        self.modules.identity = Some(Arc::new(identity));
        Ok(())
    }

    /// Row 3.4.
    async fn approval_row(&mut self) -> Result<(), StartError> {
        let row = Row::Approval;
        let journal = self.journal(row)?;
        let approval = ApprovalService::new(
            journal.datastore().clone(),
            journal.clone(),
            ApprovalConfig {
                backend: earlier(&self.modules.backend, row)?,
                local_computer_name: self.inputs.computer_name.clone(),
                approvers: None,
            },
        )
        .await
        .map_err(failed(row))?;
        journal.subscribe(approval.event_sink());
        journal.register_prune_participant(approval.prune_participant());
        self.modules.approval = Some(approval);
        Ok(())
    }

    /// Row 3.5.
    async fn localisation_row(&mut self) -> Result<(), StartError> {
        let row = Row::Localisation;
        let localisation = Localisation::start(
            LocalisationConfig {
                store: PreferenceStore::Datastore(self.datastore(row)?),
                os_locales: self.inputs.os_locales.clone(),
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
            earlier(&self.modules.profile, row)?,
        )
        .await
        .map_err(failed(row))?;
        self.modules.localisation = Some(localisation);
        Ok(())
    }

    /// Row 3.6.
    async fn agent_folders_row(&mut self) -> Result<(), StartError> {
        let row = Row::AgentFolders;
        let journal = (*self.journal(row)?).clone();
        let folders = AgentFolderManager::new(journal, self.data_root())
            .await
            .map_err(failed(row))?;
        self.modules.agent_folders = Some(Arc::new(folders));
        Ok(())
    }

    /// Row 3.8, without a part source (R18).
    fn resolver_row(&mut self) -> Result<(), StartError> {
        let resolver = ResolverAndCompiler::new(ResolverConfig {
            bundle_root: self.data_root().join("bundles"),
            compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
        })
        .map_err(failed(Row::Resolver))?;
        self.modules.resolver = Some(Arc::new(resolver));
        Ok(())
    }

    /// Row 3.9.
    fn sharepoint_row(&mut self) -> Result<(), StartError> {
        let row = Row::SharePoint;
        let adapter = SharePointAdapter::new(SharePointDeps {
            identity: earlier(&self.modules.identity, row)?,
            journal: (*self.journal(row)?).clone(),
            http: self.inputs.http.clone(),
        })
        .map_err(failed(row))?;
        // DSK-JRN-4: a journeys build reaches the SharePoint stand-in.
        #[cfg(feature = "journeys")]
        let adapter = match journeys::sharepoint_graph(&self.data_root()) {
            Some(graph) => adapter
                .with_stand_in(&graph, 1.0, journeys::SHAREPOINT_TRANSFER)
                .map_err(failed(row))?,
            None => adapter,
        };
        self.modules.sharepoint = Some(Arc::new(adapter));
        Ok(())
    }

    /// Row 3.12, with no harness until `set_harnesses` (UsagePlanner late binding).
    async fn usage_row(&mut self) -> Result<(), StartError> {
        let row = Row::UsagePlanner;
        let journal = self.journal(row)?;
        let harnesses: Vec<Arc<dyn HarnessModels>> = Vec::new();
        let usage = UsagePlanner::new(
            journal.datastore().clone(),
            journal.clone(),
            earlier(&self.modules.profile, row)?,
            harnesses,
        )
        .await
        .map_err(failed(row))?;
        // DSK-AGM-1: the person's own models and efforts, from the first step.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
        usage.set_personal_choices(self.agent_preferences().choices());
        journal.register_prune_participant(usage.prune_participant());
        self.modules.usage = Some(usage);
        Ok(())
    }

    /// Row 3.13; started at step 4 with the sender of row 3.49.
    fn connector_row(&mut self) -> Result<(), StartError> {
        let row = Row::ServerConnector;
        let server = earlier(&self.modules.profile, row)?
            .current()
            .document
            .server
            .clone();
        let client_certificate = if server.client_certificate_required {
            server.client_certificate_name
        } else {
            None
        };
        let identity: Arc<IdentityService> = earlier(&self.modules.identity, row)?;
        let connector = ServerConnector::new(
            ConnectorConfig {
                computer_name: self.inputs.computer_name.clone(),
                client_certificate,
                read_cache_bytes: DEFAULT_READ_CACHE_BYTES,
            },
            (*self.journal(row)?).clone(),
            identity,
        )
        .map_err(failed(row))?;
        self.modules.connector = Some(Arc::new(connector));
        Ok(())
    }

    /// Row 3.14, with the late-bound `RulesHost`.
    fn rules_row(&mut self) -> Result<(), StartError> {
        let row = Row::Rules;
        let journal = self.journal(row)?;
        let root = self.data_root();
        let host = Arc::new(DesktopRulesHost {
            agent_folders: earlier(&self.modules.agent_folders, row)?,
            localisation: earlier(&self.modules.localisation, row)?,
            late: self.late.clone(),
        });
        let rules = RulesEngine::new(
            journal.datastore().clone(),
            journal,
            earlier(&self.modules.profile, row)?,
            earlier(&self.modules.approval, row)?,
            host,
            RulesConfig {
                ordin_servers: ordin_servers(),
                ordin_server_prefixes: vec!["craft_".to_owned()],
                protected_roots: protected_roots(&self.inputs.home, &root),
                app_data_root: root,
                program: ordin_rules::ProgramKind::Desktop,
                instance_name: "local".to_owned(),
            },
        );
        let _ = self.late.rules.set(rules.clone());
        self.modules.rules = Some(rules);
        Ok(())
    }

    /// Row 3.15.
    async fn brand_row(&mut self) -> Result<(), StartError> {
        let row = Row::Brand;
        let brand = Brand::start(
            BrandConfig {
                datastore: self.datastore(row)?,
                data_root: self.data_root(),
                http: self.inputs.http.clone(),
            },
            earlier(&self.modules.profile, row)?,
            earlier(&self.modules.localisation, row)?,
        )
        .await
        .map_err(failed(row))?;
        self.modules.brand = Some(brand);
        Ok(())
    }

    /// Row 3.17, without a part source (R18).
    fn checker_row(&mut self) -> Result<(), StartError> {
        let row = Row::Checker;
        let checker = Checker::new(
            earlier(&self.modules.resolver, row)?,
            earlier(&self.modules.profile, row)?,
            self.journal(row)?,
        );
        self.modules.checker = Some(Arc::new(checker));
        Ok(())
    }

    /// Row 3.18.
    fn installers_row(&mut self) -> Result<(), StartError> {
        let row = Row::Installers;
        let installers = Installers::new(
            earlier(&self.modules.agent_folders, row)?,
            earlier(&self.modules.resolver, row)?,
        );
        self.modules.installers = Some(Arc::new(installers));
        Ok(())
    }

    /// Settings › Agents and models as stored in `preferences.json` (DSK-AGM-1).
    fn agent_preferences(&self) -> crate::agent_settings::AgentPreferences {
        crate::prefs::Preferences::new(self.inputs.paths.preferences.clone())
            .read()
            .agents
    }

    /// The harness chosen at row 3.19, else worked out the same way.
    fn harness_in_use(
        &self,
        snapshot: &ordin_organisation_profile::ProfileSnapshot,
    ) -> HarnessName {
        self.modules.harness_in_use.unwrap_or_else(|| {
            crate::agent_settings::harness_to_use(&self.agent_preferences(), snapshot)
        })
    }

    /// Row 3.19: no Run Queue on the desktop; adapters are registered at rows 3.32–3.34.
    fn supervisor_row(&mut self) -> Result<(), StartError> {
        let row = Row::SessionSupervisor;
        let journal = self.journal(row)?;
        let snapshot = earlier(&self.modules.profile, row)?.current();
        // The person's harness when this version can run it, else the one the profile names first
        // (SessionSupervisor.md `local_adapter`; DSK-AGM-1).
        // spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
        let harness = crate::agent_settings::harness_to_use(&self.agent_preferences(), &snapshot);
        self.modules.harness_in_use = Some(harness);
        let local_adapter = match harness {
            HarnessName::Copilot => AdapterKind::CopilotLocal,
            HarnessName::ClaudeCode => AdapterKind::ClaudeLocal,
        };
        let wall_clock_minutes = snapshot.document.limits.run.wall_clock_minutes;
        let host = Arc::new(DesktopSupervisorHost {
            late: self.late.clone(),
        });
        let supervisor = SessionSupervisor::new(
            journal.datastore().clone(),
            journal.clone(),
            earlier(&self.modules.usage, row)?,
            None,
            host,
            SupervisorConfig {
                program: ordin_session_supervisor::ProgramKind::Desktop,
                local_adapter,
                local_session_limit: 8,
                instance_name: "local".to_owned(),
                default_wall_clock_secs: wall_clock_minutes.saturating_mul(60),
                idle_timeout_secs: 600,
                stop_deadline: Duration::from_secs(10),
            },
        );
        journal.subscribe(supervisor.event_sink());
        journal.register_prune_participant(supervisor.prune_participant());
        let _ = self.late.supervisor.set(supervisor.clone());
        self.modules.supervisor = Some(supervisor);
        Ok(())
    }

    /// Row 3.20: providers are registered at rows 3.27–3.47.
    fn mcp_front_row(&mut self) -> Result<(), StartError> {
        let row = Row::McpFront;
        let front = McpFront::new(
            McpFrontConfig {
                app_data_root: self.data_root(),
                installation_id: self.inputs.installation_id,
                hook_program: self.inputs.paths.hook_program.clone(),
                profile: ProgramProfile::Desktop,
                ..McpFrontConfig::default()
            },
            earlier(&self.modules.rules, row)?,
            (*self.journal(row)?).clone(),
            Arc::new(StandardBlockCheck),
            None,
        );
        self.modules.mcp_front = Some(Arc::new(front));
        Ok(())
    }

    /// Row 3.21.
    fn inspector_row(&mut self) -> Result<(), StartError> {
        let row = Row::PackageInspector;
        let inspector = PackageInspector::new(
            earlier(&self.modules.rules, row)?,
            self.data_root().join("inspections"),
        )
        .map_err(failed(row))?;
        let inspector = Arc::new(inspector);
        let _ = self.late.inspector.set(inspector.clone());
        self.modules.inspector = Some(inspector);
        Ok(())
    }

    /// Row 3.22.
    async fn files_row(&mut self) -> Result<(), StartError> {
        let row = Row::Files;
        let journal = self.journal(row)?;
        let files = FileService::new(FileServiceDeps {
            journal: journal.clone(),
            rules: earlier(&self.modules.rules, row)?,
            agent_folders: earlier(&self.modules.agent_folders, row)?,
            data_root: self.data_root(),
        })
        .await
        .map_err(failed(row))?;
        journal.register_prune_participant(files.prune_participant());
        let _ = self.late.files.set(files.clone());
        self.modules.files = Some(files);
        Ok(())
    }

    /// Row 3.23 over the File Service (DSK-WIRE-1), checked before its wiring (DSK-WIRE-2).
    fn hook_relay_row(&mut self) -> Result<(), StartError> {
        let row = Row::HookRelay;
        let journal = self.journal(row)?;
        let relay = HookRelay::new(
            journal.datastore().clone(),
            journal.clone(),
            earlier(&self.modules.rules, row)?,
            earlier(&self.modules.approval, row)?,
            earlier(&self.modules.supervisor, row)?,
            Arc::new(FileAgentWrites {
                files: earlier(&self.modules.files, row)?,
            }),
            HookRelayConfig {
                app_data_root: self.data_root(),
                installation_id: self.inputs.installation_id.to_string(),
            },
        );
        check_attribution(&relay)?;
        journal.subscribe(relay.event_sink());
        self.modules.hook_relay = Some(relay);
        Ok(())
    }

    /// Row 3.24, with the late-bound `CrewHost`.
    fn crew_row(&mut self) -> Result<(), StartError> {
        let row = Row::CrewOrchestrator;
        let journal = self.journal(row)?;
        let profile = earlier(&self.modules.profile, row)?;
        let host = Arc::new(DesktopCrewHost {
            late: self.late.clone(),
            localisation: earlier(&self.modules.localisation, row)?,
            profile: profile.clone(),
            mcp_front: earlier(&self.modules.mcp_front, row)?,
        });
        let orchestrator = CrewOrchestrator::new(
            journal.datastore().clone(),
            journal.clone(),
            earlier(&self.modules.supervisor, row)?,
            earlier(&self.modules.rules, row)?,
            earlier(&self.modules.approval, row)?,
            earlier(&self.modules.usage, row)?,
            profile,
            host,
        );
        journal.register_prune_participant(orchestrator.prune_participant());
        let _ = self.late.orchestrator.set(orchestrator.clone());
        self.modules.orchestrator = Some(orchestrator);
        Ok(())
    }

    /// Row 3.27: `Sandbox::start` over the MCP Front, the program's `SandboxGrants` (SPEC-GAP-37) and
    /// `PlaceFiles`, Identity and the Journal; the idle limit is the profile's
    /// `sandbox.session_idle_minutes`. The Sandbox never fails start: a fault leaves it offering no
    /// tools, with its status logged by the Sandbox itself.
    // spec: ordin-desktop/components/Sandbox#SA-RECON-05
    #[cfg(feature = "sandbox")]
    async fn sandbox_row(&mut self) -> Result<(), StartError> {
        let row = Row::Sandbox;
        let idle_minutes = earlier(&self.modules.profile, row)?
            .current()
            .document
            .sandbox
            .session_idle_minutes;
        let front = earlier(&self.modules.mcp_front, row)?;
        let sandbox = ordin_sandbox::Sandbox::start(ordin_sandbox::SandboxDeps {
            dispatch: front,
            grants: Arc::new(crate::hosts_sandbox::DesktopSandboxGrants),
            identity: earlier(&self.modules.identity, row)?,
            journal: (*self.journal(row)?).clone(),
            place_files: Arc::new(crate::hosts_sandbox::DesktopPlaceFiles {
                files: earlier(&self.modules.files, row)?,
                agent_folders: earlier(&self.modules.agent_folders, row)?,
            }),
            session_idle: Duration::from_secs(u64::from(idle_minutes) * 60),
            data_root: self.data_root(),
            log_level: "info".to_owned(),
        })
        .await;
        self.modules.sandbox = Some(sandbox);
        Ok(())
    }

    /// Row 3.28 (SPEC-GAP-14).
    async fn memory_row(&mut self) -> Result<(), StartError> {
        let row = Row::MemoryService;
        let memory = MemoryService::start(
            MemoryServiceConfig {
                program: ordin_memory_service::ProgramKind::Desktop,
                models_dir: self.inputs.models_dir.clone(),
                agent_wide_scope: None,
                log_level: tracing::Level::INFO,
                recall_log_days: 90,
                knowledge_scope: None,
                model_manifest: self.inputs.model_manifest.clone(),
            },
            self.journal(row)?,
            self.shutdown.clone(),
        )
        .await
        .map_err(failed(row))?;
        // DSK-MEM-1: Settings › Memory as saved, before the first write or recall.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MEM-1
        let saved = crate::prefs::Preferences::new(self.inputs.paths.preferences.clone())
            .read()
            .memory;
        if let Err(e) = memory.set_settings(saved.settings()) {
            tracing::warn!(
                component = "desktop",
                code = e.code(),
                "memory_settings_not_applied"
            );
        }
        let memory = Arc::new(memory);
        let _ = self.late.memory.set(memory.clone());
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-4
        let models = crate::memory_models::MemoryModels::start(crate::memory_models::ModelsDeps {
            source: self.inputs.models_source.clone(),
            manifest: self.inputs.model_manifest.clone(),
            models_dir: self.inputs.models_dir.clone(),
            work_dir: self.inputs.paths.data_root.join("models.download"),
            http: self.inputs.http.clone(),
            gate: memory.clone(),
            events: Arc::new(crate::memory_models::JournalModelsEvents::new(
                self.journal(row)?,
                self.shutdown.clone(),
            )),
            shutdown: self.shutdown.clone(),
            rules: crate::memory_models::DownloadRules::default(),
        });
        self.modules.memory_models = Some(models);
        self.modules.memory = Some(memory);
        Ok(())
    }

    /// Row 3.29.
    async fn craft_row(&mut self) -> Result<(), StartError> {
        let row = Row::CraftToolkit;
        let Some(definitions) = CraftToolDefinitions::bundled() else {
            return Err(StartError::ModuleFailed {
                module: row.module(),
                code: START_FAILED.to_owned(),
            });
        };
        let craft = CraftToolkit::new(CraftDeps {
            journal: (*self.journal(row)?).clone(),
            registry: earlier(&self.modules.mcp_front, row)?,
            rules: earlier(&self.modules.rules, row)?,
            files: earlier(&self.modules.files, row)?,
            profile: earlier(&self.modules.profile, row)?,
            http: self.inputs.http.clone(),
            data_root: self.data_root(),
            platform: craft_platform(),
            definitions,
        })
        .await
        .map_err(failed(row))?;
        self.modules.craft = Some(craft);
        Ok(())
    }

    /// Row 3.30, with no Word → Markdown conversion until C-13 confirms it.
    fn confluence_row(&mut self) -> Result<(), StartError> {
        let row = Row::Confluence;
        let journal = self.journal(row)?;
        let confluence = ConfluenceAdapter::new(ConfluenceDeps {
            journal: journal.clone(),
            identity: earlier(&self.modules.identity, row)?,
            approvals: earlier(&self.modules.approval, row)?,
            files: earlier(&self.modules.files, row)?,
            http: self.inputs.http.clone(),
            settings: confluence_settings(&*earlier(&self.modules.profile, row)?),
            word_to_markdown: None,
        });
        journal.subscribe(confluence.event_sink());
        #[cfg(feature = "journeys")]
        journeys::confluence(&confluence, &self.data_root());
        self.modules.confluence = Some(confluence);
        Ok(())
    }

    /// Row 3.30a.
    fn jira_row(&mut self) -> Result<(), StartError> {
        let row = Row::Jira;
        let deps = JiraDeps {
            journal: self.journal(row)?,
            identity: earlier(&self.modules.identity, row)?,
            http: self.inputs.http.clone(),
        };
        #[cfg(feature = "journeys")]
        let jira = journeys::jira(deps, &self.data_root());
        #[cfg(not(feature = "journeys"))]
        let jira = JiraAdapter::new(deps);
        self.modules.jira = Some(jira);
        Ok(())
    }

    /// Row 3.31, with the late-bound `ServerInstaller` (3.43) and `StreamSink` (3.49).
    async fn projects_row(&mut self) -> Result<(), StartError> {
        let row = Row::Projects;
        let projects = ProjectService::new(ProjectDeps {
            journal: self.journal(row)?,
            tool_registry: earlier(&self.modules.mcp_front, row)?,
            files: earlier(&self.modules.files, row)?,
            rules: earlier(&self.modules.rules, row)?,
            identity: earlier(&self.modules.identity, row)?,
            installer: Arc::new(SetupLanguageServers {
                late: self.late.clone(),
            }),
            streams: Arc::new(BridgeStreams::new(self.late.clone())),
            settings: project_settings(&*earlier(&self.modules.profile, row)?),
            data_root: self.data_root(),
        })
        .await
        .map_err(failed(row))?;
        // DSK-SSH-1: the SSH key GitHub's git traffic uses, as saved, before the first fetch.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
        ordin_projects::ssh::set_chosen_key(
            crate::prefs::Preferences::new(self.inputs.paths.preferences.clone())
                .read()
                .github_ssh_key,
        );
        self.modules.projects = Some(projects);
        Ok(())
    }

    /// Row 3.32, with the program's `SessionSetupSource` (DSK-WIRE-3) and no `SessionRecordStore`.
    fn claude_row(&mut self) -> Result<(), StartError> {
        let row = Row::ClaudeAdapter;
        let tested_versions = earlier(&self.modules.profile, row)?
            .current()
            .document
            .harnesses
            .iter()
            .find(|h| h.name == HarnessName::ClaudeCode)
            .map(|h| h.tested_versions.clone())
            .unwrap_or_default();
        let setup = Arc::new(DesktopSessionSetup {
            late: self.late.clone(),
            installers: earlier(&self.modules.installers, row)?,
            usage: earlier(&self.modules.usage, row)?,
            agent_folders: earlier(&self.modules.agent_folders, row)?,
            journal: earlier(&self.modules.journal, row)?,
            files: earlier(&self.modules.files, row)?,
            projects: earlier(&self.modules.projects, row)?,
        });
        let claude = ClaudeLocalAdapter::new(
            ClaudeAdapterConfig {
                claude_path: self.inputs.claude_path.clone(),
                hook_program: self.inputs.paths.hook_program.clone(),
                app_data_root: self.data_root(),
                profile: ProgramProfile::Desktop,
                tested_versions,
                sign_in: SignInMode::PersonSignIn,
                stop_deadline: Duration::from_secs(10),
            },
            earlier(&self.modules.mcp_front, row)?,
            earlier(&self.modules.hook_relay, row)?,
            earlier(&self.modules.rules, row)?,
            setup,
            None,
        )
        .map_err(failed(row))?;
        self.modules.claude = Some(Arc::new(claude));
        Ok(())
    }

    /// Row 3.33 (DSK-CPX-4): the adapter asks the program for the person's Copilot CLI at every
    /// session start and probe call; `CliCopilotPort` starts it from the `CliServerSpec` the adapter
    /// passes, never from a path beside the executable.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CPX-4
    #[cfg(feature = "copilot")]
    fn copilot_row(&mut self) -> Result<(), StartError> {
        use ordin_copilot_adapter::{CliCopilotPort, CopilotAdapterConfig, CopilotLocalAdapter};
        let row = Row::CopilotAdapter;
        let tested_versions = earlier(&self.modules.profile, row)?
            .current()
            .document
            .harnesses
            .iter()
            .find(|h| h.name == HarnessName::Copilot)
            .map(|h| h.tested_versions.clone())
            .unwrap_or_default();
        let setup = Arc::new(crate::hosts_run::DesktopCopilotSessionSetup {
            inner: Arc::new(DesktopSessionSetup {
                late: self.late.clone(),
                installers: earlier(&self.modules.installers, row)?,
                usage: earlier(&self.modules.usage, row)?,
                agent_folders: earlier(&self.modules.agent_folders, row)?,
                journal: earlier(&self.modules.journal, row)?,
                files: earlier(&self.modules.files, row)?,
                projects: earlier(&self.modules.projects, row)?,
            }),
        });
        let copilot = CopilotLocalAdapter::new(
            CopilotAdapterConfig {
                tested_versions,
                app_data_root: self.data_root(),
                profile: ProgramProfile::Desktop,
                stop_deadline: Duration::from_secs(10),
            },
            Arc::new(CliCopilotPort::new()),
            Arc::new(crate::hosts_copilot::DesktopCopilotCli {
                lookup: crate::hosts_copilot::CopilotLookup::from_process(),
            }),
            earlier(&self.modules.mcp_front, row)?,
            earlier(&self.modules.hook_relay, row)?,
            earlier(&self.modules.rules, row)?,
            earlier(&self.modules.identity, row)?,
            setup,
        )
        .map_err(|e| StartError::ModuleFailed {
            module: row.module(),
            code: e.code().to_owned(),
        })?;
        self.modules.copilot = Some(Arc::new(copilot));
        Ok(())
    }

    /// `setup_assistant.register_probe(<Copilot HarnessProbe>)`, right after row 3.43 (DSK-CPX-3,
    /// DSK-CPX-5). Nothing is located at start: with no Copilot CLI installed, start completes and
    /// nothing is logged for Copilot.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CPX-3
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CPX-5
    #[cfg(feature = "copilot")]
    fn register_copilot_probe(&mut self) -> Result<(), StartError> {
        let setup = earlier(&self.modules.setup, Row::Setup)?;
        let adapter = earlier(&self.modules.copilot, Row::CopilotAdapter)?;
        setup.register_probe(Arc::new(crate::hosts_copilot::DesktopCopilotProbe {
            lookup: crate::hosts_copilot::CopilotLookup::from_process(),
            adapter,
        }));
        Ok(())
    }

    /// Row 3.35.
    async fn workspaces_row(&mut self) -> Result<(), StartError> {
        let row = Row::Workspaces;
        let journal = self.journal(row)?;
        #[cfg(feature = "journeys")]
        journeys::microsoft_sign_in(&earlier(&self.modules.identity, row)?, &self.data_root())
            .await;
        let workspaces = WorkspaceService::new(WorkspaceDeps {
            journal: (*journal).clone(),
            rules: earlier(&self.modules.rules, row)?,
            files: earlier(&self.modules.files, row)?,
            sharepoint: earlier(&self.modules.sharepoint, row)?,
            confluence: Some(earlier(&self.modules.confluence, row)?),
            jira: Some(earlier(&self.modules.jira, row)?),
            data_root: self.data_root(),
            documents_dir: self.inputs.documents_dir.clone(),
            on_server: false,
        })
        .await
        .map_err(failed(row))?;
        journal.subscribe(workspaces.clone());
        let _ = self.late.workspaces.set(workspaces.clone());
        self.modules.workspaces = Some(workspaces);
        Ok(())
    }

    /// Row 3.36.
    fn catalogue_row(&mut self) -> Result<(), StartError> {
        let row = Row::Catalogue;
        let projects = earlier(&self.modules.projects, row)?;
        let catalogue = CatalogueService::new(
            CatalogueDeps {
                datastore: self.datastore(row)?,
                profile: earlier(&self.modules.profile, row)?,
                github: projects as Arc<dyn GithubAccess>,
                formats: Arc::new(FormatPlugins::builtin()),
                checker: earlier(&self.modules.checker, row)?,
            },
            CatalogueConfig {
                root: self.data_root().join("catalogue"),
                bundled_catalogue: self.inputs.bundled_catalogue.clone(),
                ordin_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        );
        self.modules.catalogue = Some(Arc::new(catalogue));
        Ok(())
    }

    /// R18: the Catalogue Service is the one part source of the Resolver and the Checker.
    fn bind_part_source(&mut self) -> Result<(), StartError> {
        let row = Row::Catalogue;
        let source: Arc<dyn PartSource> = earlier(&self.modules.catalogue, row)?;
        earlier(&self.modules.resolver, row)?
            .install_part_source(source.clone())
            .map_err(failed(row))?;
        earlier(&self.modules.checker, row)?
            .install_part_source(source)
            .map_err(failed(row))?;
        Ok(())
    }

    /// Row 3.37.
    fn push_row(&mut self) -> Result<(), StartError> {
        let row = Row::Push;
        let journal = self.journal(row)?;
        let projects = earlier(&self.modules.projects, row)?;
        let push = PushService::new(PushDeps {
            journal: journal.clone(),
            approvals: earlier(&self.modules.approval, row)?,
            working_copy: Arc::new(DesktopWorkingCopy {
                workspaces: earlier(&self.modules.workspaces, row)?,
                files: earlier(&self.modules.files, row)?,
            }),
            files: earlier(&self.modules.files, row)?,
            projects: projects.clone(),
            github: projects as Arc<dyn GithubAccess>,
            sharepoint: earlier(&self.modules.sharepoint, row)?,
            confluence: Some(earlier(&self.modules.confluence, row)?),
            craft: earlier(&self.modules.craft, row)?,
            localisation: earlier(&self.modules.localisation, row)?,
        });
        journal.subscribe(push.event_sink());
        let _ = self.late.push.set(push.clone());
        self.modules.push = Some(push);
        Ok(())
    }

    /// Row 3.38: `ProgramKind::Desktop`, no agent-profile ports.
    async fn knowledge_row(&mut self) -> Result<(), StartError> {
        let row = Row::Knowledge;
        let workspaces = earlier(&self.modules.workspaces, row)?;
        let knowledge = KnowledgeService::new(
            ordin_knowledge::ProgramKind::Desktop,
            KnowledgeDeps {
                journal: self.journal(row)?,
                files: Arc::new(DesktopKnowledgeFiles {
                    files: earlier(&self.modules.files, row)?,
                    agent_folders: earlier(&self.modules.agent_folders, row)?,
                }),
                workspaces: Arc::new(DesktopKnowledgeWorkspaces {
                    workspaces: workspaces.clone(),
                    late: self.late.clone(),
                }),
                rules: Arc::new(DesktopKnowledgeRules {
                    rules: earlier(&self.modules.rules, row)?,
                    files: earlier(&self.modules.files, row)?,
                    workspaces,
                    late: self.late.clone(),
                }),
                memory: Arc::new(DesktopKnowledgeMemory::new(earlier(
                    &self.modules.memory,
                    row,
                )?)),
                push: Arc::new(DesktopKnowledgePush {
                    push: earlier(&self.modules.push, row)?,
                }),
                agent: None,
                folders: Arc::new(NoKnowledgeFolders),
                identity: Arc::new(DesktopKnowledgeIdentity {
                    identity: earlier(&self.modules.identity, row)?,
                }),
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
            },
        )
        .await
        .map_err(failed(row))?;
        let _ = self.late.knowledge.set(knowledge.clone());
        self.modules.knowledge = Some(knowledge);
        Ok(())
    }

    /// Row 3.41a: the Team Knowledge Service over its owners (DSK-TW-2). Its run hook's sink is
    /// registered by the service's own `run` (TeamKnowledgeService.md TW-F step 1), started at step 4.
    #[cfg(feature = "team-knowledge")]
    fn team_knowledge_row(&mut self) -> Result<(), StartError> {
        use crate::hosts_team::*;
        let row = Row::TeamKnowledge;
        let journal = self.journal(row)?;
        let packages = earlier(&self.modules.packages, row)?;
        let projects = earlier(&self.modules.projects, row)?;
        let service = ordin_team_knowledge::TeamKnowledgeService::new(
            ordin_team_knowledge::TeamKnowledgeDeps {
                journal,
                memory: Arc::new(DesktopTeamMemory {
                    memory: earlier(&self.modules.memory, row)?,
                }),
                pages: Arc::new(DesktopTeamPages {
                    knowledge: earlier(&self.modules.knowledge, row)?,
                }),
                push: Arc::new(DesktopTeamPush {
                    push: earlier(&self.modules.push, row)?,
                }),
                workspaces: Arc::new(DesktopTeamWorkspaces {
                    workspaces: earlier(&self.modules.workspaces, row)?,
                }),
                repos: Arc::new(DesktopTeamRepos {
                    projects: projects.clone(),
                    identity: earlier(&self.modules.identity, row)?,
                }),
                agents: Arc::new(DesktopTeamAgents {
                    packages: packages.clone(),
                    catalogue: earlier(&self.modules.catalogue, row)?,
                    projects,
                    localisation: earlier(&self.modules.localisation, row)?,
                }),
                runs: Arc::new(DesktopTeamRuns {
                    supervisor: earlier(&self.modules.supervisor, row)?,
                    packages,
                    inspector: earlier(&self.modules.inspector, row)?,
                }),
                clones_dir: self.data_root().join("team-knowledge"),
            },
        );
        self.modules.team_knowledge = Some(service);
        Ok(())
    }

    /// Row 3.16a (DSK-MIN-1): the Transcription Service over `<data root>/speech-models`.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
    #[cfg(feature = "minutes")]
    fn transcription_row(&mut self) -> Result<(), StartError> {
        let row = Row::Transcription;
        let localisation = earlier(&self.modules.localisation, row)?;
        self.modules.transcription =
            Some(ordin_dictation::transcription::TranscriptionService::new(
                self.data_root().join("speech-models"),
                localisation,
                self.inputs.http.clone(),
            ));
        Ok(())
    }

    /// Row 3.16b (DSK-MIN-1): the OBS Recorder over the Identity Service's stored password.
    #[cfg(feature = "minutes")]
    fn obs_row(&mut self) -> Result<(), StartError> {
        let row = Row::ObsRecorder;
        self.modules.obs = Some(ordin_obs::ObsRecorder::new(
            earlier(&self.modules.identity, row)?,
            earlier(&self.modules.localisation, row)?,
        ));
        Ok(())
    }

    /// Row 3.41a (DSK-MIN-1): the Minutes Service over `<data root>/minutes`, Ordin writing the
    /// minutes through `MinutesWriterOverHost`. `run` is spawned at step 4.
    #[cfg(feature = "minutes")]
    fn minutes_row(&mut self) -> Result<(), StartError> {
        let row = Row::Minutes;
        let service = ordin_minutes::MinutesService::new(
            self.data_root().join("minutes"),
            earlier(&self.modules.transcription, row)?,
            earlier(&self.modules.obs, row)?,
            self.journal(row)?,
            earlier(&self.modules.localisation, row)?,
            Arc::new(crate::hosts::MinutesWriterOverHost {
                host: earlier(&self.modules.ordin_host, row)?,
            }),
        )
        .map_err(|e| StartError::ModuleFailed {
            module: row.module(),
            code: ordin_types::CodedError::code(&e).to_owned(),
        })?;
        // DSK-MIN-6: Ordin's minutes and transcription tools reach the service through the port.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-6
        earlier(&self.modules.ordin_host, row)?.set_minutes_port(Arc::new(
            crate::hosts::MinutesPortOverService {
                minutes: service.clone(),
                journal: self.journal(row)?,
                files: earlier(&self.modules.files, row)?,
                localisation: earlier(&self.modules.localisation, row)?,
            },
        ));
        self.modules.minutes = Some(service);
        Ok(())
    }

    /// Row 3.40.
    fn packages_row(&mut self) -> Result<(), StartError> {
        let row = Row::PackageCatalogue;
        let journal = self.journal(row)?;
        let packages = PackageCatalogue::new(PackageCatalogueDeps {
            datastore: journal.datastore().clone(),
            journal,
            catalogue: earlier(&self.modules.catalogue, row)?,
            resolver: earlier(&self.modules.resolver, row)?,
            checker: earlier(&self.modules.checker, row)?,
            installers: earlier(&self.modules.installers, row)?,
            folders: earlier(&self.modules.agent_folders, row)?,
            inspector: earlier(&self.modules.inspector, row)?,
            harness: Arc::new(ProfileHarness {
                profile: earlier(&self.modules.profile, row)?,
                chosen: self.modules.harness_in_use,
            }),
            activity: Arc::new(SupervisorActivity {
                late: self.late.clone(),
            }),
        });
        let packages = Arc::new(packages);
        let _ = self.late.packages.set(packages.clone());
        self.modules.packages = Some(packages);
        Ok(())
    }

    /// Row 3.41, with the program's `RefResolver`; the two instance ids are the bundled agents'
    /// fixed ids, so this row reads nothing for them (DSK-REC-23).
    fn ordin_host_row(&mut self) -> Result<(), StartError> {
        let row = Row::OrdinHost;
        let journal = self.journal(row)?;
        let snapshot = earlier(&self.modules.profile, row)?.current();
        let refs = Arc::new(DesktopRefResolver::new(
            journal.clone(),
            earlier(&self.modules.catalogue, row)?,
            &*earlier(&self.modules.localisation, row)?,
            self.late.clone(),
        ));
        let host = OrdinHost::new(
            OrdinHostConfig {
                // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-23
                ordin_agent_instance_id: BundledAgent::Ordin.instance_id(),
                agent_maker_instance_id: BundledAgent::AgentMaker.instance_id(),
                harness: harness_kind(self.harness_in_use(&snapshot)),
                profile: ProgramProfile::Desktop,
                // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-21
                fixed_instructions_version: ordin_host::FIXED_INSTRUCTIONS_VERSION.to_owned(),
                coordinator_turn_timeout: Duration::from_secs(10 * 60),
                repository_check_period: Duration::from_secs(15 * 60),
                max_waiting_messages: 5,
            },
            host_policy(
                &snapshot.document,
                earlier(&self.modules.brand, row)?.coordinator_name(),
            ),
            OrdinHostDeps {
                supervisor: earlier(&self.modules.supervisor, row)?,
                orchestrator: earlier(&self.modules.orchestrator, row)?,
                usage: earlier(&self.modules.usage, row)?,
                inspector: earlier(&self.modules.inspector, row)?,
                catalogue: earlier(&self.modules.packages, row)?,
                memory: earlier(&self.modules.memory, row)?,
                knowledge: earlier(&self.modules.knowledge, row)?,
                push: earlier(&self.modules.push, row)?,
                projects: earlier(&self.modules.projects, row)?,
                workspaces: earlier(&self.modules.workspaces, row)?,
                journal: journal.clone(),
                datastore: journal.datastore().clone(),
                blocks: Arc::new(StandardBlockCheck),
                files: earlier(&self.modules.files, row)?,
                refs: refs.clone(),
                localisation: earlier(&self.modules.localisation, row)?,
            },
        )
        .map_err(failed(row))?;
        journal.register_prune_participant(host.prune_participant());
        let _ = self.late.ordin_host.set(host.clone());
        self.modules.refs = Some(refs);
        self.modules.ordin_host = Some(host);
        Ok(())
    }

    /// `mcp_front.bind_block_store(ordin_host.block_store())` (row 3.41, SA-MCP-BLOCK-02).
    fn bind_block_store(&mut self) -> Result<(), StartError> {
        let row = Row::OrdinHost;
        let store = earlier(&self.modules.ordin_host, row)?.block_store();
        earlier(&self.modules.mcp_front, row)?
            .bind_block_store(store)
            .map_err(failed(Row::McpFront))
    }

    /// Row 3.42, then `register_source` for agents, workflows and conversations.
    async fn home_row(&mut self) -> Result<(), StartError> {
        let row = Row::HomeSearch;
        let journal = self.journal(row)?;
        let reader: Arc<dyn ordin_home_search::JournalReader> = Arc::new(DesktopJournalReader {
            journal: journal.clone(),
            approval: earlier(&self.modules.approval, row)?,
            late: self.late.clone(),
            localisation: self.modules.localisation.clone(),
        });
        self.modules.journal_reader = Some(reader.clone());
        let home = HomeSearch::new(HomeSearchDeps {
            journal,
            reader,
            knowledge: earlier(&self.modules.knowledge, row)?.reader(),
            files: earlier(&self.modules.files, row)?,
            workspaces: earlier(&self.modules.workspaces, row)?,
            sharepoint: earlier(&self.modules.sharepoint, row)?,
            confluence: Some(earlier(&self.modules.confluence, row)?),
        })
        .await
        .map_err(failed(row))?;
        let sources: [Arc<dyn SearchSource>; 3] = [
            Arc::new(AgentSearch {
                late: self.late.clone(),
            }),
            Arc::new(WorkflowSearch {
                late: self.late.clone(),
            }),
            Arc::new(ConversationSearch {
                late: self.late.clone(),
            }),
        ];
        for source in sources {
            home.register_source(source);
        }
        self.modules.home = Some(home);
        Ok(())
    }

    /// Row 3.43 (DSK-UP-5: `bundled_programs` by switch).
    fn setup_row(&mut self) -> Result<(), StartError> {
        let row = Row::Setup;
        let bundled_programs = vec![BundledProgram {
            name: "the ordin hook".to_owned(),
            publisher: "gatblau".to_owned(),
            path: self.inputs.paths.hook_program.clone(),
            version_args: vec!["--version".to_owned()],
            expected_sha256: None,
            restore_from: None,
            needed_for: "agent sessions".to_owned(),
        }];
        let setup = SetupAssistant::new(
            SetupDeps {
                profile: earlier(&self.modules.profile, row)?,
                identity: earlier(&self.modules.identity, row)?,
                catalogue: earlier(&self.modules.catalogue, row)?,
                packages: earlier(&self.modules.packages, row)?,
                craft: earlier(&self.modules.craft, row)?,
            },
            SetupConfig {
                program: SetupProgram::Desktop,
                bundled_programs,
                application_version: env!("CARGO_PKG_VERSION").to_owned(),
                http: self.inputs.http.clone(),
            },
            self.datastore(row)?,
        )
        .map_err(failed(row))?;
        let setup = Arc::new(setup);
        let _ = self.late.setup.set(setup.clone());
        self.modules.setup = Some(setup);
        Ok(())
    }

    /// `setup_assistant.register_updater(TauriUpdater)`; without the shell there is none.
    fn register_updater(&mut self) -> Result<(), StartError> {
        let setup = earlier(&self.modules.setup, Row::Setup)?;
        if let Some(updater) = self.inputs.updater.clone() {
            setup.register_updater(updater);
        }
        Ok(())
    }

    /// `mcp_front.register(<provider>.tool_provider())`. The program does not shorten an owner's
    /// tool text; the Ordin Host's descriptions fit the MCP Front's limit.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-22
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-31
    fn register_provider(&mut self, provider: Provider) -> Result<(), StartError> {
        let (row, tools) = match provider {
            #[cfg(feature = "sandbox")]
            Provider::Sandbox => (
                Row::Sandbox,
                earlier(&self.modules.sandbox, Row::Sandbox)?.tool_provider(),
            ),
            Provider::MemoryService => (
                Row::MemoryService,
                earlier(&self.modules.memory, Row::MemoryService)?.tool_provider(),
            ),
            Provider::CraftToolkit => (
                Row::CraftToolkit,
                earlier(&self.modules.craft, Row::CraftToolkit)?.tool_provider(),
            ),
            Provider::Projects => (
                Row::Projects,
                earlier(&self.modules.projects, Row::Projects)?.tool_provider(),
            ),
            Provider::Knowledge => (
                Row::Knowledge,
                earlier(&self.modules.knowledge, Row::Knowledge)?.tool_provider(),
            ),
            Provider::OrdinHost => (
                Row::OrdinHost,
                earlier(&self.modules.ordin_host, Row::OrdinHost)?.tool_provider(),
            ),
            #[allow(unreachable_patterns)]
            _ => {
                return Err(StartError::ModuleFailed {
                    module: "ordin-desktop",
                    code: START_FAILED.to_owned(),
                })
            }
        };
        earlier(&self.modules.mcp_front, row)?
            .register(tools)
            .map_err(failed(row))?;
        self.modules.providers.push(provider);
        Ok(())
    }

    /// `session_supervisor.register_adapter(<adapter>)` (rows 3.32–3.34).
    fn register_adapter(&mut self, adapter: Adapter) -> Result<(), StartError> {
        let (row, runtime): (Row, Arc<dyn RuntimeAdapter>) = match adapter {
            Adapter::Claude => (
                Row::ClaudeAdapter,
                earlier(&self.modules.claude, Row::ClaudeAdapter)?,
            ),
            #[cfg(feature = "copilot")]
            Adapter::Copilot => (
                Row::CopilotAdapter,
                earlier(&self.modules.copilot, Row::CopilotAdapter)?,
            ),
            #[allow(unreachable_patterns)]
            _ => {
                return Err(StartError::ModuleFailed {
                    module: "ordin-desktop",
                    code: START_FAILED.to_owned(),
                })
            }
        };
        earlier(&self.modules.supervisor, row)?
            .register_adapter(runtime)
            .map_err(failed(row))?;
        self.modules.adapters.push(adapter);
        Ok(())
    }

    /// `usage.set_harnesses(..)`: `copilot` off, the Claude Local Adapter's only; `copilot` on, both
    /// local adapters' (DSK-UP-5).
    fn set_harnesses(&mut self) -> Result<(), StartError> {
        let row = Row::UsagePlanner;
        let claude = earlier(&self.modules.claude, Row::ClaudeAdapter)?;
        #[allow(unused_mut)]
        let mut harnesses = vec![claude.harness_models()];
        #[cfg(feature = "copilot")]
        harnesses.push(earlier(&self.modules.copilot, Row::CopilotAdapter)?.harness_models());
        earlier(&self.modules.usage, row)?.set_harnesses(harnesses);
        Ok(())
    }

    /// `mcp_front.bind_ref_resolver(<RefResolver>)` (SA-CLOSE-01).
    fn bind_ref_resolver(&mut self) -> Result<(), StartError> {
        let row = Row::McpFront;
        let refs: Arc<dyn RefResolver> = earlier(&self.modules.refs, row)?;
        earlier(&self.modules.mcp_front, row)?
            .bind_ref_resolver(refs)
            .map_err(failed(row))
    }

    /// `mcp_front.start(shutdown)`, which freezes the registry.
    async fn start_mcp_front(&mut self) -> Result<(), StartError> {
        let row = Row::McpFront;
        earlier(&self.modules.mcp_front, row)?
            .start(self.shutdown.clone())
            .await
            .map_err(failed(row))?;
        self.modules.mcp_started = true;
        Ok(())
    }

    /// Row 3.48: every module that answers a command, `<data root>/preferences.json`, and the
    /// window shell in place of the `AppHandle`.
    fn service_api_row(&mut self) -> Result<(), StartError> {
        let row = Row::ServiceApi;
        let sentences = Arc::new(DesktopErrorSentences {
            localisation: earlier(&self.modules.localisation, row)?,
        });
        let api = DesktopServiceApi::new(
            ApiModules {
                backend: self.modules.backend,
                journal: self.modules.journal.clone(),
                approval: self.modules.approval.clone(),
                localisation: self.modules.localisation.clone(),
                catalogue: self.modules.catalogue.clone(),
                checker: self.modules.checker.clone(),
                confluence: self.modules.confluence.clone(),
                connector: self.modules.connector.clone(),
                craft: self.modules.craft.clone(),
                files: self.modules.files.clone(),
                home: self.modules.home.clone(),
                journal_reader: self.modules.journal_reader.clone(),
                identity: self.modules.identity.clone(),
                inspector: self.modules.inspector.clone(),
                knowledge: self.modules.knowledge.clone(),
                memory: self.modules.memory.clone(),
                memory_models: self.modules.memory_models.clone(),
                orchestrator: self.modules.orchestrator.clone(),
                ordin_host: self.modules.ordin_host.clone(),
                packages: self.modules.packages.clone(),
                profile: self.modules.profile.clone(),
                projects: self.modules.projects.clone(),
                push: self.modules.push.clone(),
                setup: self.modules.setup.clone(),
                installers: self.modules.installers.clone(),
                resolver: self.modules.resolver.clone(),
                sharepoint: self.modules.sharepoint.clone(),
                jira: self.modules.jira.clone(),
                supervisor: self.modules.supervisor.clone(),
                usage: self.modules.usage.clone(),
                workspaces: self.modules.workspaces.clone(),
                brand: self.modules.brand.clone(),
                shell: self.inputs.shell.clone(),
                preferences: Some(Arc::new(crate::prefs::Preferences::new(
                    self.inputs.paths.preferences.clone(),
                ))),
                url_opener: self.inputs.url_opener.clone(),
                harness_in_use: self.modules.harness_in_use,
                #[cfg(feature = "team-knowledge")]
                team_knowledge: self.modules.team_knowledge.clone(),
                #[cfg(feature = "minutes")]
                minutes: self.modules.minutes.clone(),
            },
            sentences.clone(),
        );
        self.modules.sentences = Some(sentences);
        self.modules.service_api = Some(Arc::new(api));
        Ok(())
    }

    /// Row 3.49: the bridge, its sender kept for step 4, and `bridge.start(cancel)`. `app.manage`
    /// belongs to the shell.
    fn bridge_row(&mut self) -> Result<(), StartError> {
        let row = Row::CommandBridge;
        let (bridge, sender) = CommandBridge::new(
            BridgeConfig {
                local_backend: earlier(&self.modules.backend, row)?,
                server_replay_events: DEFAULT_SERVER_REPLAY_EVENTS,
            },
            earlier(&self.modules.service_api, row)?,
            earlier(&self.modules.sentences, row)?,
            earlier(&self.modules.connector, row)?,
            (*self.journal(row)?).clone(),
        );
        bridge.start(self.shutdown.clone());
        let _ = self.late.streams.set(bridge.streams());
        self.modules.bridge = Some(Arc::new(bridge));
        self.modules.server_events = Some(sender);
        Ok(())
    }
}

#[async_trait::async_trait]
impl StepRunner for ModuleRunner {
    async fn run(&mut self, step: Step) -> Result<(), StartError> {
        match step {
            Step::Construct(row) => match row {
                Row::Journal => self.journal_row().await,
                Row::OrganisationProfile => self.profile_row().await,
                Row::Identity => self.identity_row().await,
                Row::Approval => self.approval_row().await,
                Row::Localisation => self.localisation_row().await,
                Row::AgentFolders => self.agent_folders_row().await,
                // Row 3.7: a library; nothing is constructed.
                Row::FormatPlugins => Ok(()),
                Row::Resolver => self.resolver_row(),
                Row::SharePoint => self.sharepoint_row(),
                Row::UsagePlanner => self.usage_row().await,
                Row::ServerConnector => self.connector_row(),
                Row::Rules => self.rules_row(),
                Row::Brand => self.brand_row().await,
                Row::Checker => self.checker_row(),
                Row::Installers => self.installers_row(),
                Row::SessionSupervisor => self.supervisor_row(),
                Row::McpFront => self.mcp_front_row(),
                Row::PackageInspector => self.inspector_row(),
                Row::Files => self.files_row().await,
                Row::HookRelay => self.hook_relay_row(),
                Row::CrewOrchestrator => self.crew_row(),
                #[cfg(feature = "sandbox")]
                Row::Sandbox => self.sandbox_row().await,
                Row::MemoryService => self.memory_row().await,
                Row::CraftToolkit => self.craft_row().await,
                Row::Confluence => self.confluence_row(),
                Row::Jira => self.jira_row(),
                Row::Projects => self.projects_row().await,
                Row::ClaudeAdapter => self.claude_row(),
                #[cfg(feature = "copilot")]
                Row::CopilotAdapter => self.copilot_row(),
                Row::Workspaces => self.workspaces_row().await,
                Row::Catalogue => self.catalogue_row(),
                Row::Push => self.push_row(),
                Row::Knowledge => self.knowledge_row().await,
                Row::PackageCatalogue => self.packages_row(),
                Row::OrdinHost => self.ordin_host_row(),
                #[cfg(feature = "team-knowledge")]
                Row::TeamKnowledge => self.team_knowledge_row(),
                // Row 3.16: `ordin-dictation` holds no Dictation Service yet, only the Transcription
                // Service of row 3.16a; nothing is constructed and the `dictation.*` commands answer
                // `SVC_NOT_IN_THIS_VERSION` (plan 07 FU-015).
                #[cfg(feature = "dictation")]
                Row::Dictation => Ok(()),
                #[cfg(feature = "minutes")]
                Row::Transcription => self.transcription_row(),
                #[cfg(feature = "minutes")]
                Row::ObsRecorder => self.obs_row(),
                #[cfg(feature = "minutes")]
                Row::Minutes => self.minutes_row(),
                Row::HomeSearch => self.home_row().await,
                Row::Setup => self.setup_row(),
                Row::ServiceApi => self.service_api_row(),
                Row::CommandBridge => self.bridge_row(),
                // A row behind a build switch: not constructed yet.
                #[allow(unreachable_patterns)]
                other => Err(StartError::ModuleFailed {
                    module: other.module(),
                    code: START_FAILED.to_owned(),
                }),
            },
            Step::RegisterProvider(provider) => self.register_provider(provider),
            Step::RegisterAdapter(adapter) => self.register_adapter(adapter),
            Step::SetHarnesses => self.set_harnesses(),
            Step::BindPartSource => self.bind_part_source(),
            Step::BindBlockStore => self.bind_block_store(),
            Step::BindRefResolver => self.bind_ref_resolver(),
            Step::StartMcpFront => self.start_mcp_front().await,
            Step::RegisterUpdater => self.register_updater(),
            #[cfg(feature = "copilot")]
            Step::RegisterCopilotProbe => self.register_copilot_probe(),
            #[allow(unreachable_patterns)]
            _ => Err(StartError::ModuleFailed {
                module: "ordin-desktop",
                code: START_FAILED.to_owned(),
            }),
        }
    }
}

/// A journeys test build only (OrdinDesktop.md DSK-JRN-4): the online services' stand-ins named in
/// `<data root>/journeys-stand-ins.json`. Never in a release (DSK-JRN-2).
#[cfg(feature = "journeys")]
mod journeys {
    use std::path::Path;

    use std::sync::Arc;
    use std::time::Duration;

    use ordin_confluence::{ConfluenceAdapter, ConfluenceTestSeam};
    use ordin_identity::{IdentityService, SignInState};
    use ordin_jira::{JiraAdapter, JiraDeps, JiraEndpoints};
    use ordin_types::domain::identity::CredentialKind;
    use ordin_types::error::CodedError as _;

    /// The stand-in's transfer timeout: Graph's own 120 s, unscaled.
    pub(super) const SHAREPOINT_TRANSFER: Duration = Duration::from_secs(120);

    pub(super) const FILE: &str = "journeys-stand-ins.json";

    /// The file's object, or `None` when it is absent or is not a JSON object.
    pub(super) fn read(data_root: &Path) -> Option<serde_json::Map<String, serde_json::Value>> {
        let text = std::fs::read_to_string(data_root.join(FILE)).ok()?;
        match serde_json::from_str(&text) {
            Ok(serde_json::Value::Object(map)) => Some(map),
            _ => {
                tracing::warn!(target: "ordin_desktop", file = FILE, "journeys.stand_ins_unreadable");
                None
            }
        }
    }

    /// `confluence: { gateway, site, token }` → the adapter's test seam (CONF-SEAM-2).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-JRN-4
    pub(super) fn confluence(adapter: &ConfluenceAdapter, data_root: &Path) {
        let Some(entry) = read(data_root).and_then(|m| m.get("confluence").cloned()) else {
            return;
        };
        let text = |k: &str| {
            entry
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        let Ok(site) = text("site").parse() else {
            tracing::warn!(target: "ordin_desktop", service = "confluence", "journeys.stand_in_refused");
            return;
        };
        let seam = ConfluenceTestSeam {
            gateway: text("gateway"),
            site,
            token: text("token"),
        };
        match adapter.use_test_seam(seam) {
            Ok(()) => {
                tracing::info!(target: "ordin_desktop", service = "confluence", "journeys.stand_in")
            }
            Err(e) => {
                tracing::warn!(target: "ordin_desktop", service = "confluence", code = e.code(), "journeys.stand_in_refused")
            }
        }
    }

    fn entry(data_root: &Path, service: &str) -> Option<serde_json::Value> {
        read(data_root).and_then(|m| m.get(service).cloned())
    }

    fn text(entry: &serde_json::Value, key: &str) -> Option<String> {
        entry.get(key).and_then(|v| v.as_str()).map(str::to_owned)
    }

    /// `sharepoint: { graph }` → the stand-in Graph address the row gives `with_stand_in`.
    pub(super) fn sharepoint_graph(data_root: &Path) -> Option<String> {
        let graph = entry(data_root, "sharepoint").and_then(|e| text(&e, "graph"))?;
        if reqwest::Url::parse(&graph).is_err() {
            tracing::warn!(target: "ordin_desktop", service = "sharepoint", "journeys.stand_in_refused");
            return None;
        }
        tracing::info!(target: "ordin_desktop", service = "sharepoint", "journeys.stand_in");
        Some(graph)
    }

    /// `sharepoint: { token_endpoint, client_id }` → a signed-in Microsoft account whose tokens come from
    /// the stand-in, holding SharePoint's scopes, when none is signed in yet or the one signed in lacks
    /// them (`seed_microsoft_for_tests`).
    pub(super) async fn microsoft_sign_in(identity: &Arc<IdentityService>, data_root: &Path) {
        let Some(e) = entry(data_root, "sharepoint") else {
            return;
        };
        let (Some(token_endpoint), Some(client_id)) =
            (text(&e, "token_endpoint"), text(&e, "client_id"))
        else {
            return;
        };
        if matches!(
            identity.sign_in_state(CredentialKind::Microsoft).await,
            Ok(SignInState::SignedIn)
        ) {
            // A sign-in seeded before it carried SharePoint's scopes is replaced; any other is kept.
            match identity
                .access_token_for(CredentialKind::Microsoft, &ordin_sharepoint::SCOPES)
                .await
            {
                Err(err) if err.code() == "CRED_SCOPE_MISSING" => {
                    let old = identity.list().await.unwrap_or_default();
                    for c in old.iter().filter(|c| c.kind == CredentialKind::Microsoft) {
                        let _ = identity.sign_out(c.id).await;
                    }
                }
                _ => return,
            }
        }
        match identity
            .seed_microsoft_for_tests(
                "journeys-refresh",
                &client_id,
                &token_endpoint,
                &ordin_sharepoint::SCOPES,
            )
            .await
        {
            Ok(_) => {
                tracing::info!(target: "ordin_desktop", service = "microsoft", "journeys.stand_in")
            }
            Err(err) => {
                tracing::warn!(target: "ordin_desktop", service = "microsoft", code = err.code(), "journeys.stand_in_refused")
            }
        }
    }

    /// `jira: { site_base_url, gateway_base_url }` → the adapter's endpoints (`JiraEndpoints`).
    pub(super) fn jira(deps: JiraDeps, data_root: &Path) -> Arc<JiraAdapter> {
        let Some(e) = entry(data_root, "jira") else {
            return JiraAdapter::new(deps);
        };
        tracing::info!(target: "ordin_desktop", service = "jira", "journeys.stand_in");
        JiraAdapter::with_endpoints(
            deps,
            JiraEndpoints {
                site_base_url: text(&e, "site_base_url"),
                gateway_base_url: text(&e, "gateway_base_url"),
                request_timeout: None,
            },
        )
    }
}
