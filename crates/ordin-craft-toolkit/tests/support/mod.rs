//! The integration rig: the real Journal, Rules Engine, Approval Service, File Service, Organisation
//! Profile and MCP Front over SQLite on a temporary file, through their public surfaces only.
//!
//! The profile is the real service loading a signed cached document, because its fetch accepts
//! `https` addresses only. The real ArtCraft apps are not available, so the app is a shell program
//! the test writes, its contract taken from CraftToolkit.md (Internal Logic 5.2, 6): `<cli> mcp`
//! serves MCP as newline-delimited JSON-RPC over stdio. The row and the folder are laid out as
//! `sync_installations` leaves them, because a download needs an `https` address.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use ed25519_dalek::{Signer as _, SigningKey};
use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_craft_toolkit::{
    AppWindow, CraftAppDefinition, CraftDeps, CraftPlatform, CraftToolDefinition,
    CraftToolDefinitions, CraftToolkit, PathArg, PathRole,
};
use ordin_files::{AddPlace, FileService, FileServiceDeps, Place, PlaceKind};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry,
    ModuleMigrations, SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{McpFront, McpFrontConfig, ProgramProfile, SessionGrant, ToolCaller};
use ordin_organisation_profile::{
    Clock, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
    MODULE_NAME, SQLITE_MIGRATIONS,
};
use ordin_rules::{
    AgentToolPolicy, CardWording, ControlLine, FactError, GrantedPlace, InstanceRuleSet,
    MessageRule, ProgramKind, Proposal, RulesConfig, RulesEngine, RulesHost, RunKey, RunStartFacts,
};
use ordin_service_api::blocks::{EntityRef, PlanState, RefResolver, StandardBlockCheck};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::identity::HarnessKind;
use ordin_types::domain::places::{FolderAccess, Grantee};
use ordin_types::encoding::b64url_encode;
use ordin_types::num::Sha256Hex;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{
    AgentInstanceId, BackendId, CraftInstallationId, EmailLinkId, OrganisationProfileId, RunId,
    WorkspaceId,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;

/// A folder removed when dropped. A short name: a Unix socket path is limited to about 100 bytes.
pub struct TempDir(pub PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let dir = std::env::temp_dir().join(format!("oct-{}", &id[..8]));
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

#[async_trait]
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
        Ok(1)
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

struct Resolver;

#[async_trait]
impl RefResolver for Resolver {
    async fn content(&self, _: WorkspaceId, _: &ordin_types::wire::ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        true
    }
    async fn entity(&self, _: WorkspaceId, _: EntityRef) -> bool {
        true
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        Some("Analyst".to_owned())
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        PlanState {
            versions: 0,
            current_len: 0,
        }
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

/// The profile document: `photocraft` 0.7.0 approved, `png` mapped to it, `crop` not allowed.
pub fn profile_document() -> Vec<u8> {
    let asset = json!({
        "asset_url": "https://mirror.acme.co.uk/craft/photocraft/0.7.0/p.zip",
        "sha256": "a".repeat(64),
        "file_name": "p.zip"
    });
    serde_json::to_vec(&json!({
        "format_version": 1,
        "profile_version": "1.0.0",
        "craft": {
            "apps": [{
                "app_id": "photocraft",
                "version": "0.7.0",
                "allowed_tools": ["resize", "crop"],
                "platforms": { "linux_x86_64": asset, "macos_arm64": asset }
            }],
            "file_types": { "png": "photocraft" }
        }
    }))
    .expect("the document")
}

/// `installer.json`, the cached document and signature, and the active row, as a refresh leaves them.
async fn cache_profile(data_root: &Path, pool: &sqlx::SqlitePool) {
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let document = profile_document();
    let header = b64url_encode(br#"{"alg":"EdDSA"}"#);
    let input = format!("{header}.{}", b64url_encode(&document));
    let signature = b64url_encode(&key.sign(input.as_bytes()).to_bytes());
    let jws = format!("{header}..{signature}");
    let installer = json!({
        "profile_signing_keys": [b64url_encode(key.verifying_key().as_bytes())]
    });
    std::fs::write(data_root.join("installer.json"), installer.to_string()).expect("installer");
    let cache = data_root.join("profile-cache");
    std::fs::create_dir_all(&cache).expect("cache folder");
    std::fs::write(cache.join("active.json"), &document).expect("document");
    std::fs::write(cache.join("active.jws"), &jws).expect("signature");
    let digest: [u8; 32] = Sha256::digest(&document).into();
    let now = Timestamp::now().to_wire();
    sqlx::query(
        "INSERT INTO organisation_profiles (id, profile_version, signature, document_digest, \
         cached_path, state, fetched_at, verified_at, activated_at) \
         VALUES (?1, '1.0.0', ?2, ?3, ?4, 'active', ?5, ?5, ?5)",
    )
    .bind(OrganisationProfileId::new().to_string())
    .bind(&jws)
    .bind(Sha256Hex::from_digest(&digest).0)
    .bind(cache.to_string_lossy().into_owned())
    .bind(now)
    .execute(pool)
    .await
    .expect("the active row");
}

/// `(tool, shell command)` of the fake app's MCP mode.
fn fake_app(tools: &[(&str, &str)]) -> String {
    let listed: Vec<String> = tools
        .iter()
        .map(|(name, _)| {
            format!(
                r#"{{"name":"{name}","description":"From the app.","inputSchema":{{"type":"object","properties":{{"width":{{"type":"number"}}}}}}}}"#
            )
        })
        .collect();
    let arms: String = tools
        .iter()
        .map(|(name, does)| format!("        {name}) {does} ;;\n"))
        .collect();
    let listed = listed.join(",");
    format!(
        r#"#!/bin/sh
[ "$1" = "mcp" ] || exit 2
arg() {{ printf '%s' "$line" | sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }}
reply() {{ printf '{{"jsonrpc":"2.0","id":%s,"result":%s}}\n' "$id" "$1"; }}
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      reply '{{"protocolVersion":"2025-06-18","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"fake","version":"1.0"}}}}' ;;
    *'"method":"notifications/'*) ;;
    *'"method":"tools/list"'*)
      reply '{{"tools":[{listed}]}}' ;;
    *'"method":"tools/call"'*)
      output=$(arg output)
      name=$(printf '%s' "$line" | sed -n 's/.*"params":{{.*"name":"\([^"]*\)".*/\1/p')
      case "$name" in
{arms}      esac ;;
  esac
done
"#
    )
}

fn definitions() -> CraftToolDefinitions {
    let tool = |name: &str, paths: &[(&str, PathRole)]| CraftToolDefinition {
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
    };
    let paths = [("input", PathRole::Input), ("output", PathRole::Output)];
    CraftToolDefinitions {
        format: 1,
        apps: vec![CraftAppDefinition {
            app: "photocraft".to_owned(),
            versions: "^0.7".to_owned(),
            cli: "photocraft-cli".to_owned(),
            window: AppWindow {
                macos_bundle: Some("PhotoCraft.app".to_owned()),
                windows_exe: Some("PhotoCraft.exe".to_owned()),
                linux_exe: Some("photocraft".to_owned()),
            },
            tools: vec![
                tool("resize", &paths),
                tool("crop", &paths),
                tool("export", &paths),
            ],
        }],
    }
}

pub struct Stack {
    pub dir: TempDir,
    pub journal: Arc<Journal>,
    pub engine: Arc<RulesEngine>,
    pub files: Arc<FileService>,
    pub front: McpFront,
    pub toolkit: Arc<CraftToolkit>,
    pub place: Place,
    pub place_root: PathBuf,
    pub store: Datastore,
}

pub struct Session {
    pub run: RunId,
    pub token: String,
    pub endpoint: String,
}

impl Session {
    /// Connects a bridge for `server` with this session's token.
    pub async fn connect(&self, server: &str) -> Bridge {
        let stream = UnixStream::connect(&self.endpoint)
            .await
            .expect("the socket");
        let (read, write) = stream.into_split();
        let mut bridge = Bridge {
            reader: BufReader::new(read),
            writer: write,
        };
        bridge
            .send(json!({
                "protocol": 1,
                "session": self.run.to_string(),
                "server": server,
                "token": self.token,
            }))
            .await;
        assert_eq!(bridge.recv().await, json!({"kind": "ready"}));
        bridge
    }
}

pub struct Bridge {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

impl Bridge {
    async fn send(&mut self, value: Value) {
        self.writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .expect("write");
    }

    async fn recv(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(20), self.reader.read_line(&mut line))
            .await
            .expect("a line within the wait")
            .expect("read");
        serde_json::from_str(line.trim()).expect("a JSON line")
    }

    async fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let line = self.recv().await;
            if line.get("id") == Some(&json!(id)) {
                return line;
            }
        }
    }

    pub async fn tools(&mut self) -> Vec<String> {
        let answer = self.request(1000, "tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }

    pub async fn call(&mut self, id: i64, tool: &str, arguments: Value) -> Value {
        self.request(
            id,
            "tools/call",
            json!({"name": tool, "arguments": arguments}),
        )
        .await
    }
}

/// The text of a tools/call answer.
pub fn text_of(answer: &Value) -> String {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

pub fn is_error(answer: &Value) -> bool {
    answer["result"]["isError"].as_bool().unwrap_or(false)
}

pub const WRITES_OUTPUT: &str = r#"printf 'PNGDATA' > "$output"; reply '{"content":[{"type":"text","text":"wrote '"$output"'"}]}'"#;

impl Stack {
    /// A running stack with `photocraft` installed and serving `tools`.
    pub async fn start(tools: &[(&str, &str)]) -> Stack {
        let dir = TempDir::new();
        let data_root = dir.0.join("data");
        std::fs::create_dir_all(&data_root).expect("data root");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: data_root.join("journal.db"),
            backup_dir: data_root.join("backups"),
        })
        .await
        .expect("open the SQLite datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(ModuleMigrations {
                module: MODULE_NAME,
                sqlite: SQLITE_MIGRATIONS,
                postgres: &[],
            })
            .expect("profile");
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
        let pool = store.sqlite().expect("SQLite").clone();
        cache_profile(&data_root, &pool).await;
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
        let http = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client");
        let profile = Arc::new(
            OrganisationProfileService::start(
                ProfileMode::Read {
                    pool: pool.clone(),
                    data_root: data_root.clone(),
                    http: http.clone(),
                    build_defaults: InstallerSettings::default(),
                },
                ProfileOptions {
                    refresh_interval: Duration::from_secs(3600),
                    clock: Arc::new(WallClock),
                },
            )
            .await
            .expect("profile service"),
        );
        let engine = RulesEngine::new(
            store.clone(),
            journal.clone(),
            profile.clone(),
            approvals,
            Arc::new(Host),
            RulesConfig {
                ordin_servers: vec!["ordin".to_owned(), "work".to_owned()],
                ordin_server_prefixes: vec!["craft_".to_owned()],
                protected_roots: vec![dir.0.join("protected")],
                app_data_root: data_root.clone(),
                program: ProgramKind::Desktop,
                instance_name: "local".to_owned(),
            },
        );
        let agent_folders = Arc::new(
            AgentFolderManager::new(journal.as_ref().clone(), data_root.clone())
                .await
                .expect("agent folder manager"),
        );
        let files = FileService::new(FileServiceDeps {
            journal: journal.clone(),
            rules: engine.clone(),
            agent_folders,
            data_root: data_root.clone(),
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

        // The state `sync_installations` leaves behind.
        let folder = data_root.join("craft/photocraft/0.7.0");
        std::fs::create_dir_all(&folder).expect("version folder");
        let cli = folder.join("photocraft-cli");
        std::fs::write(&cli, fake_app(tools)).expect("the program");
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).expect("mode");
        sqlx::query(
            "INSERT INTO craft_installations (id, app_name, pinned_version, installed_version, \
             installed_sha256, verification_result, cli_modes, status, updated_at) \
             VALUES (?1, 'photocraft', '0.7.0', '0.7.0', ?2, 'verified', '[\"mcp\"]', \
             'installed', ?3)",
        )
        .bind(CraftInstallationId::new().to_string())
        .bind("a".repeat(64))
        .bind(Timestamp::now().to_wire())
        .execute(&pool)
        .await
        .expect("the row");

        let front = McpFront::new(
            McpFrontConfig {
                app_data_root: dir.0.clone(),
                installation_id: uuid::Uuid::now_v7(),
                hook_program: PathBuf::from("/Applications/ordin.app/Contents/MacOS/ordin-hook"),
                ..McpFrontConfig::default()
            },
            engine.clone(),
            journal.as_ref().clone(),
            Arc::new(StandardBlockCheck),
            None,
        );
        let toolkit = CraftToolkit::new(CraftDeps {
            journal: journal.as_ref().clone(),
            registry: Arc::new(front.clone()),
            rules: engine.clone(),
            files: files.clone(),
            profile,
            http,
            data_root,
            platform: CraftPlatform::LinuxX86_64,
            definitions: definitions(),
        })
        .await
        .map_err(|e| e.code())
        .expect("the toolkit");
        toolkit.start().await.expect("the toolkit starts");
        front.register(toolkit.tool_provider()).expect("registers");
        front.bind_ref_resolver(Arc::new(Resolver)).expect("binds");
        front.start(CancellationToken::new()).await.expect("starts");
        Stack {
            dir,
            journal,
            engine,
            files,
            front,
            toolkit,
            place,
            place_root,
            store,
        }
    }

    pub fn agent(&self, deny: &[&str]) -> AgentInstanceId {
        let id = AgentInstanceId::new();
        self.engine
            .load_rule_set(InstanceRuleSet {
                agent_instance_id: id,
                package_name: "analyst".to_owned(),
                authority: Authority::SystemChanging,
                agent_folder: self.dir.0.join("agents/eng-1"),
                agent_folder_read_only: false,
                declaration: None,
                configured: None,
                tool_policy: Some(AgentToolPolicy {
                    prohibited_verbs: deny.iter().map(|s| (*s).to_owned()).collect(),
                    ..Default::default()
                }),
                approval_list: Vec::new(),
                package_read_only_tools: Vec::new(),
                tools: Vec::new(),
                unmapped_operations: Vec::new(),
                external: Vec::new(),
                commands: Vec::new(),
                reviewer: None,
            })
            .expect("the set loads");
        id
    }

    pub async fn open(&self, deny: &[&str]) -> (Session, SessionGrant) {
        let agent = self.agent(deny);
        let run = RunId::new();
        let workspace = WorkspaceId::new();
        self.engine.replace_grants(
            workspace,
            vec![GrantedPlace {
                grantee: Grantee::AgentInstance {
                    agent_instance_id: agent,
                },
                place_id: self.place.id,
                root: self.place_root.clone(),
                access: FolderAccess::ReadAndChange,
            }],
        );
        self.engine
            .begin_run(RunStartFacts {
                run_id: run,
                agent_instance_id: agent,
                workspace_id: workspace,
                crew_charter_id: None,
                command: None,
                text: String::new(),
                scope: None,
                asker: None,
                started_by: "person".to_owned(),
                started_by_id: "person:asha".to_owned(),
            })
            .await
            .expect("the run begins");
        let grant = SessionGrant {
            run_id: run,
            workspace_id: workspace,
            agent_instance_id: agent,
            harness: HarnessKind::ClaudeCode,
            profile: ProgramProfile::Desktop,
            caller: ToolCaller::Agent,
            scope_id: None,
            coding_task_id: None,
        };
        let config = self
            .front
            .open_session(grant.clone())
            .await
            .expect("open_session");
        let launch = config
            .servers
            .iter()
            .find(|s| s.name.as_str() == "craft_photocraft")
            .expect("the server is launched");
        let session = Session {
            run,
            token: launch.env["ORDIN_MCP_TOKEN"].clone(),
            endpoint: launch.env["ORDIN_MCP_ENDPOINT"].clone(),
        };
        (session, grant)
    }

    pub fn in_place(&self, relative: &str) -> String {
        self.place_root
            .join(relative)
            .to_string_lossy()
            .into_owned()
    }
}
