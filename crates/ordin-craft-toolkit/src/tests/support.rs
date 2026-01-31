//! The test rig.
//!
//! The real ArtCraft apps are not available and C-07, C-21, C-28 and C-47 are unconfirmed, so each
//! app here is a small shell program the test writes. Its contract is CraftToolkit.md's own text:
//! `<cli> --version` and `<cli> --help` (Internal Logic 2.3.5, 2.3.6) and `<cli> mcp` serving MCP
//! as newline-delimited JSON-RPC over stdio (Approach, Internal Logic 5.2).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_files::{AddPlace, FileService, FileServiceDeps, Place, PlaceKind};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{
    CallVia, McpError, ProgramProfile, ProgressSink, ServerName, SessionGrant, ToolCallContext,
    ToolCaller, ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolRegistry,
};
use ordin_organisation_profile::{
    Clock, CraftApp, CraftAsset, CraftSection, OrganisationProfileService, ProfileMode,
    ProfileOptions, ProfilePlatform, ProfileSnapshot, Timestamp,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, GrantedPlace, InstanceRuleSet, MessageRule, ProgramKind,
    Proposal, RulesConfig, RulesEngine, RulesHost, RunKey,
};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::identity::HarnessKind;
use ordin_types::domain::places::{FolderAccess, Grantee};
use ordin_types::num::Sha256Hex;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, RunId, ToolRequestId, WorkspaceId};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::definitions::{
    AppWindow, CraftAppDefinition, CraftToolDefinition, CraftToolDefinitions, PathArg, PathRole,
};
use crate::model::{CraftDeps, CraftPlatform};
use crate::opening::Launcher;
use crate::store::{self, Row};
use crate::toolkit::{CraftToolkit, Limits};
use crate::{CraftCliMode, CraftInstallationStatus, VerificationResult};

/// A folder removed when dropped; the path is canonical.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-craft-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(
            std::fs::canonicalize(&dir)
                .map(ordin_types::paths::plain_form)
                .expect("canonical temporary folder"),
        )
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

struct Host;

#[async_trait::async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _: &RunKey, _: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(&self, _: &RunKey, _: &str, _: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _: &RunKey, _: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _: &RunKey, _: &str, _: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _: &RunKey, _: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn runs_in_progress(&self, _: &str) -> Result<u32, FactError> {
        Ok(0)
    }
    async fn paused_run_with_command(&self, _: &[String], _: RunId) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(&self, _: Option<&str>, _: &str) -> Result<Value, FactError> {
        Ok(Value::Null)
    }
    async fn setting_rules(&self, _: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _: &RunKey, _: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "A record".to_owned(),
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
}

/// Records `tools_changed`; registration is the program's.
#[derive(Default)]
pub(crate) struct RecordingRegistry {
    pub(crate) changed: Mutex<Vec<String>>,
}

impl ToolRegistry for RecordingRegistry {
    fn register(&self, _provider: Arc<dyn ToolProvider>) -> Result<(), McpError> {
        Ok(())
    }
    fn tools_changed(&self, server: &ServerName) {
        self.changed
            .lock()
            .unwrap()
            .push(server.as_str().to_owned());
    }
    fn served_tools(&self, _server: &ServerName) -> Option<Vec<String>> {
        None
    }
}

/// Records what would open, so that no test starts a window.
#[derive(Default)]
pub(crate) struct RecordingLauncher {
    pub(crate) started: Mutex<Vec<Vec<OsString>>>,
    pub(crate) refuse: Mutex<Option<std::io::ErrorKind>>,
}

impl Launcher for RecordingLauncher {
    fn launch(&self, program: &OsStr, args: &[OsString]) -> std::io::Result<()> {
        if let Some(kind) = *self.refuse.lock().unwrap() {
            return Err(kind.into());
        }
        let mut line = vec![program.to_owned()];
        line.extend_from_slice(args);
        self.started.lock().unwrap().push(line);
        Ok(())
    }
}

pub(crate) struct Setup {
    pub(crate) limits: Limits,
    pub(crate) apps: Vec<CraftAppDefinition>,
    pub(crate) platform: CraftPlatform,
    pub(crate) craft: CraftSection,
    /// The data root under a protected folder, so that a conversion path is refused.
    pub(crate) protect_data_root: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            limits: Limits {
                call: Duration::from_secs(10),
                start: Duration::from_secs(10),
                backoff: Duration::ZERO,
                check: Duration::from_secs(30),
                offer: Duration::from_secs(10),
                sweep: Duration::from_secs(3600),
                ..Limits::default()
            },
            apps: Vec::new(),
            platform: CraftPlatform::LinuxX86_64,
            craft: CraftSection::default(),
            protect_data_root: false,
        }
    }
}

pub(crate) struct Rig {
    pub(crate) dir: TempDir,
    pub(crate) data_root: PathBuf,
    pub(crate) journal: Arc<Journal>,
    pub(crate) rules: Arc<RulesEngine>,
    pub(crate) files: Arc<FileService>,
    /// The caller of `context`: its rule set is loaded and `place` is granted to it.
    pub(crate) agent: AgentInstanceId,
    pub(crate) workspace: WorkspaceId,
    pub(crate) toolkit: Arc<CraftToolkit>,
    pub(crate) provider: Arc<dyn ToolProvider>,
    pub(crate) registry: Arc<RecordingRegistry>,
    pub(crate) launcher: Arc<RecordingLauncher>,
    pub(crate) feed: watch::Sender<Arc<ProfileSnapshot>>,
    pub(crate) place: Place,
    pub(crate) place_root: PathBuf,
    pub(crate) store: Datastore,
}

pub(crate) fn snapshot(version: &str, craft: CraftSection) -> Arc<ProfileSnapshot> {
    let mut snapshot = ProfileSnapshot::defaults();
    snapshot.document.profile_version = version.to_owned();
    snapshot.document.craft = craft;
    Arc::new(snapshot)
}

impl Rig {
    pub(crate) async fn new(setup: Setup) -> Rig {
        let dir = TempDir::new();
        let core_root = dir.0.join("core");
        let data_root = dir.0.join("data");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: core_root.join("journal.db"),
            backup_dir: core_root.join("backups"),
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
            .register(ordin_agent_folders::migrations())
            .expect("agent folders");
        registry.register(FileService::migrations()).expect("files");
        registry
            .register(CraftToolkit::migrations())
            .expect("craft");
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
        let protected = if setup.protect_data_root {
            data_root.clone()
        } else {
            dir.0.join("protected")
        };
        let rules = RulesEngine::new(
            store.clone(),
            journal.clone(),
            profile.clone(),
            approvals,
            Arc::new(Host),
            RulesConfig {
                ordin_servers: vec!["ordin".to_owned()],
                ordin_server_prefixes: vec!["craft_".to_owned()],
                protected_roots: vec![protected],
                app_data_root: core_root.clone(),
                program: ProgramKind::Desktop,
                instance_name: "local".to_owned(),
            },
        );
        let agent_folders = Arc::new(
            AgentFolderManager::new(journal.as_ref().clone(), core_root.clone())
                .await
                .expect("agent folder manager"),
        );
        let files = FileService::new(FileServiceDeps {
            journal: journal.clone(),
            rules: rules.clone(),
            agent_folders,
            data_root: core_root,
        })
        .await
        .expect("file service");
        let place_root = dir.0.join("Brand");
        std::fs::create_dir_all(&place_root).expect("place folder");
        let place = files
            .add_place(AddPlace {
                path: place_root.clone(),
                friendly_name: Some("Brand".to_owned()),
                kind: PlaceKind::PersonsFolder,
            })
            .await
            .expect("add the place");
        let agent = AgentInstanceId::new();
        let workspace = WorkspaceId::new();
        rules
            .load_rule_set(rule_set(agent, &dir.0.join("agents/eng-1")))
            .expect("the rule set loads");
        rules.replace_grants(
            workspace,
            vec![granted(
                agent,
                place.id,
                &place_root,
                FolderAccess::ReadAndChange,
            )],
        );
        let (feed, receiver) = watch::channel(snapshot("1.0.0", setup.craft));
        let registry = Arc::new(RecordingRegistry::default());
        let launcher = Arc::new(RecordingLauncher::default());
        let deps = CraftDeps {
            journal: journal.as_ref().clone(),
            registry: registry.clone(),
            rules: rules.clone(),
            files: files.clone(),
            profile,
            http: reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("client"),
            data_root: data_root.clone(),
            platform: setup.platform,
            definitions: CraftToolDefinitions {
                format: 1,
                apps: setup.apps,
            },
        };
        let toolkit = CraftToolkit::build(deps, receiver, setup.limits, launcher.clone())
            .expect("the toolkit builds");
        toolkit.start().await.expect("the toolkit starts");
        let provider = toolkit.tool_provider();
        Rig {
            dir,
            data_root,
            journal,
            rules,
            files,
            agent,
            workspace,
            toolkit,
            provider,
            registry,
            launcher,
            feed,
            place,
            place_root,
            store,
        }
    }

    /// An app laid out as an install leaves it, with its row.
    pub(crate) async fn seed(
        &self,
        app: &str,
        version: &str,
        modes: &[CraftCliMode],
        fake: &FakeApp,
    ) {
        let folder = self.data_root.join("craft").join(app).join(version);
        std::fs::create_dir_all(&folder).expect("version folder");
        write_program(&folder.join(format!("{app}-cli")), &fake.script());
        let mut row = Row::approved(app, version);
        row.status = CraftInstallationStatus::Installed;
        row.verification_result = Some(VerificationResult::Verified);
        row.installed_version = Some(version.to_owned());
        // The hash `pinned` gives.
        row.installed_sha256 = Some("a".repeat(64));
        row.cli_modes = modes.to_vec();
        store::insert_row(self.pool(), &row).await.expect("the row");
        self.toolkit.reload_rows().await.expect("the rows");
    }

    pub(crate) fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub(crate) fn path_in_place(&self, relative: &str) -> String {
        self.place_root
            .join(relative)
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) fn file_in_place(&self, relative: &str, bytes: &[u8]) -> String {
        std::fs::write(self.place_root.join(relative), bytes).expect("a file in the place");
        self.path_in_place(relative)
    }

    pub(crate) fn context(&self, run_id: RunId, limit: Duration) -> ToolCallContext {
        ToolCallContext {
            run_id,
            workspace_id: self.workspace,
            agent_instance_id: self.agent,
            caller: ToolCaller::Agent,
            scope_id: None,
            coding_task_id: None,
            call_id: ToolRequestId::new(),
            correlation_id: "test".to_owned(),
            deadline: Instant::now() + limit,
            cancel: CancellationToken::new(),
            progress: ProgressSink::detached(),
            only_needed_parts: false,
            via: CallVia::Harness,
        }
    }

    pub(crate) async fn call(
        &self,
        run_id: RunId,
        app: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.provider
            .call(
                self.context(run_id, Duration::from_secs(60)),
                &key(app, tool),
                arguments,
            )
            .await
    }

    pub(crate) async fn offered(&self, run_id: RunId) -> Vec<String> {
        let mut tools: Vec<String> = self
            .provider
            .offer(&grant(run_id))
            .await
            .expect("offer")
            .into_iter()
            .map(|key| key.tool)
            .collect();
        tools.sort();
        tools
    }

    /// Journal entries of `kind`, as their `data`.
    pub(crate) async fn entries(&self, kind: &str) -> Vec<Value> {
        let page = self
            .journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.to_owned()),
                },
                None,
            )
            .await
            .expect("journal entries");
        page.items.into_iter().map(|e| e.data).collect()
    }

    pub(crate) fn changed(&self) -> Vec<String> {
        self.registry.changed.lock().unwrap().clone()
    }

    pub(crate) fn opened(&self) -> Vec<Vec<OsString>> {
        self.launcher.started.lock().unwrap().clone()
    }

    pub(crate) fn scratch_files(&self, run_id: RunId) -> Vec<PathBuf> {
        match std::fs::read_dir(
            self.data_root
                .join("craft-scratch")
                .join(run_id.to_string()),
        ) {
            Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// A rule set that adds nothing to the folder grants.
pub(crate) fn rule_set(agent: AgentInstanceId, folder: &Path) -> InstanceRuleSet {
    InstanceRuleSet {
        agent_instance_id: agent,
        package_name: "analyst".to_owned(),
        authority: Authority::SystemChanging,
        agent_folder: folder.to_path_buf(),
        agent_folder_read_only: false,
        declaration: None,
        configured: None,
        tool_policy: None,
        approval_list: Vec::new(),
        package_read_only_tools: Vec::new(),
        tools: Vec::new(),
        unmapped_operations: Vec::new(),
        external: Vec::new(),
        commands: Vec::new(),
        reviewer: None,
    }
}

pub(crate) fn granted(
    agent: AgentInstanceId,
    place_id: ordin_types::PlaceId,
    root: &Path,
    access: FolderAccess,
) -> GrantedPlace {
    GrantedPlace {
        grantee: Grantee::AgentInstance {
            agent_instance_id: agent,
        },
        place_id,
        root: root.to_path_buf(),
        access,
    }
}

pub(crate) fn key(app: &str, tool: &str) -> ToolKey {
    ToolKey {
        server: ServerName::new(format!("craft_{app}")).unwrap(),
        tool: tool.to_owned(),
    }
}

pub(crate) fn grant(run_id: RunId) -> SessionGrant {
    SessionGrant {
        run_id,
        workspace_id: WorkspaceId::new(),
        agent_instance_id: AgentInstanceId::new(),
        harness: HarnessKind::ClaudeCode,
        profile: ProgramProfile::Desktop,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: None,
    }
}

pub(crate) fn write_program(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, text).expect("the program");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("executable");
}

pub(crate) fn sha256_of(bytes: &[u8]) -> Sha256Hex {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    Sha256Hex::from_digest(&digest)
}

/// `(name, content, unix mode)` entries as a zip.
pub(crate) fn zip_of(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes, mode) in entries {
        let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
        zip.start_file(*name, options).expect("an entry");
        zip.write_all(bytes).expect("the content");
    }
    zip.finish().expect("the archive").into_inner()
}

pub(crate) fn tool(name: &str, paths: &[(&str, PathRole)]) -> CraftToolDefinition {
    CraftToolDefinition {
        tool: name.to_owned(),
        description: format!("Does {name}."),
        path_args: paths
            .iter()
            .map(|(name, role)| PathArg {
                name: (*name).to_owned(),
                role: *role,
            })
            .collect(),
        converts_markdown_to: None,
        converts_to_markdown_from: None,
        footer_arg: None,
    }
}

pub(crate) fn resize() -> CraftToolDefinition {
    tool(
        "resize",
        &[("input", PathRole::Input), ("output", PathRole::Output)],
    )
}

pub(crate) fn definition(app: &str, tools: Vec<CraftToolDefinition>) -> CraftAppDefinition {
    CraftAppDefinition {
        app: app.to_owned(),
        versions: "*".to_owned(),
        cli: format!("{app}-cli"),
        window: AppWindow {
            macos_bundle: Some("PhotoCraft.app".to_owned()),
            windows_exe: Some("PhotoCraft.exe".to_owned()),
            linux_exe: Some(app.to_owned()),
        },
        tools,
    }
}

pub(crate) fn asset(url: &str, sha256: &Sha256Hex, file_name: &str) -> CraftAsset {
    CraftAsset {
        asset_url: url.to_owned(),
        sha256: sha256.clone(),
        file_name: file_name.to_owned(),
    }
}

pub(crate) fn app(
    name: &str,
    version: &str,
    platforms: Vec<(ProfilePlatform, CraftAsset)>,
) -> CraftApp {
    CraftApp {
        app_id: name.to_owned(),
        version: version.to_owned(),
        allowed_tools: None,
        platforms: platforms.into_iter().collect(),
    }
}

/// An approved app whose pin is never downloaded.
pub(crate) fn pinned(name: &str, version: &str) -> CraftApp {
    let hash = Sha256Hex("a".repeat(64));
    let url = format!("https://mirror.acme.co.uk/{name}.zip");
    app(
        name,
        version,
        vec![(
            ProfilePlatform::LinuxX86_64,
            asset(&url, &hash, &format!("{name}.zip")),
        )],
    )
}

pub(crate) fn section(apps: Vec<CraftApp>, file_types: &[(&str, &str)]) -> CraftSection {
    CraftSection {
        apps,
        mirror_url: None,
        file_types: file_types
            .iter()
            .map(|(e, a)| ((*e).to_owned(), (*a).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

/// Mounts `bytes` at `at` on the mirror, expected `times` times, and returns the asset's address.
pub(crate) async fn serve(
    mirror: &MockServer,
    at: &str,
    status: u16,
    bytes: Vec<u8>,
    times: u64,
) -> String {
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(status).set_body_bytes(bytes))
        .expect(times)
        .mount(mirror)
        .await;
    format!("{}{at}", mirror.uri())
}

/// What one tool of a fake app does when called.
#[derive(Clone)]
pub(crate) enum Does {
    /// Writes `PNGDATA` to the `output` argument and names that path in its result.
    Write,
    /// Copies the `input` argument to the `output` argument.
    Copy,
    /// Writes eight bytes to `output`.
    WriteEight,
    /// Leaves `output` empty and reports success.
    WriteEmpty,
    /// Writes bytes that are not UTF-8.
    WriteBadUtf8,
    /// Writes its environment to `output`.
    WriteEnv,
    /// Records the app's working folder in `output`.
    WriteCwd,
    /// Reports success and writes nothing.
    Nothing,
    /// Never answers.
    Hang,
    /// An error result with this text.
    Fail(String),
    /// A JSON-RPC error.
    RpcError,
}

/// A fake app program: the tools its MCP mode lists and what each does.
#[derive(Clone)]
pub(crate) struct FakeApp {
    pub(crate) tools: Vec<(&'static str, Does)>,
    /// What `--help` lists.
    pub(crate) modes: &'static str,
    /// `mcp` never answers `initialize`.
    pub(crate) hang_at_start: bool,
    /// `tools/call` lines are appended here.
    pub(crate) record: Option<PathBuf>,
}

impl FakeApp {
    pub(crate) fn with(tools: Vec<(&'static str, Does)>) -> FakeApp {
        FakeApp {
            tools,
            modes: "run batch mcp",
            hang_at_start: false,
            record: None,
        }
    }

    pub(crate) fn script(&self) -> String {
        let listed: Vec<String> = self
            .tools
            .iter()
            .map(|(name, _)| {
                format!(
                    r#"{{"name":"{name}","description":"The text of the app.","inputSchema":{{"type":"object","properties":{{"width":{{"type":"number"}}}}}}}}"#
                )
            })
            .collect();
        let listed = listed.join(",");
        let mut arms = String::new();
        for (name, does) in &self.tools {
            arms.push_str(&format!("      {name}) {} ;;\n", does.shell()));
        }
        let hang = if self.hang_at_start {
            "exec sleep 60"
        } else {
            ""
        };
        let record = self.record.as_ref().map_or_else(String::new, |path| {
            format!("printf '%s\\n' \"$line\" >> '{}'", path.display())
        });
        format!(
            r#"#!/bin/sh
case "$1" in
  --version) echo "fake 1.0"; exit 0 ;;
  --help) echo "Commands: {modes}"; exit 0 ;;
  mcp) ;;
  *) exit 2 ;;
esac
{hang}
arg() {{ printf '%s' "$line" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }}
reply() {{ printf '{{"jsonrpc":"2.0","id":%s,"result":%s}}\n' "$id" "$1"; }}
done_reply() {{ reply '{{"content":[{{"type":"text","text":"done"}}]}}'; }}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      reply '{{"protocolVersion":"2025-06-18","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fake","version":"1.0"}}}}' ;;
    *'"method":"notifications/'*) ;;
    *'"method":"tools/list"'*)
      reply '{{"tools":[{listed}]}}' ;;
    *'"method":"tools/call"'*)
      {record}
      input=$(arg input)
      output=$(arg output)
      case "$line" in
        *'"name":"'*) name=$(printf '%s' "$line" | sed -n 's/.*"params":{{.*"name":"\([^"]*\)".*/\1/p') ;;
      esac
      case "$name" in
{arms}      esac ;;
  esac
done
"#,
            modes = self.modes,
        )
    }
}

impl Does {
    fn shell(&self) -> String {
        match self {
            Does::Write => r#"printf 'PNGDATA' > "$output"; reply '{"content":[{"type":"text","text":"wrote '"$output"'"}]}'"#.to_owned(),
            Does::Copy => r#"cat "$input" > "$output"; done_reply"#.to_owned(),
            Does::WriteEight => r#"printf '12345678' > "$output"; done_reply"#.to_owned(),
            Does::WriteEmpty => r#": > "$output"; done_reply"#.to_owned(),
            Does::WriteBadUtf8 => r#"printf '\377\376' > "$output"; done_reply"#.to_owned(),
            Does::WriteEnv => r#"env > "$output"; done_reply"#.to_owned(),
            Does::WriteCwd => r#"pwd > "$output"; done_reply"#.to_owned(),
            Does::Nothing => "done_reply".to_owned(),
            Does::Hang => "exec sleep 60".to_owned(),
            Does::Fail(text) => format!(
                r#"reply '{{"content":[{{"type":"text","text":"{text}"}}],"isError":true}}'"#
            ),
            Does::RpcError => {
                r#"printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"boom"}}\n' "$id""#
                    .to_owned()
            }
        }
    }
}
