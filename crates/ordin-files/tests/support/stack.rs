//! The File Service over the real Journal, Rules Engine and Agent Folder Manager on SQLite in a
//! temporary folder. Public surface only.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ordin_agent_folders::AgentFolderManager;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_files::{AddPlace, FileService, FileServiceDeps, Place, PlaceKind};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey,
};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{BackendId, RunId};

/// A folder removed when dropped; the path is canonical.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-files-it-{}", uuid::Uuid::now_v7()));
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
    async fn setting_value(
        &self,
        _: Option<&str>,
        _: &str,
    ) -> Result<serde_json::Value, FactError> {
        Ok(serde_json::Value::Null)
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

pub struct Stack {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub service: Arc<FileService>,
}

pub async fn stack() -> Stack {
    let dir = TempDir::new();
    let data_root = dir.0.join("data");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data_root.join("journal.db"),
        backup_dir: data_root.join("backups"),
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
    let rules = RulesEngine::new(
        store.clone(),
        journal.clone(),
        profile,
        approvals,
        Arc::new(Host),
        RulesConfig {
            ordin_servers: vec!["ordin".to_owned()],
            ordin_server_prefixes: vec![],
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
    let service = FileService::new(FileServiceDeps {
        journal: journal.clone(),
        rules,
        agent_folders,
        data_root,
    })
    .await
    .expect("file service");
    Stack {
        dir,
        store,
        journal,
        service,
    }
}

impl Stack {
    /// Makes `places/<name>` and adds it as a place of `kind`.
    pub async fn place(&self, name: &str, kind: PlaceKind) -> Place {
        let folder = self.dir.0.join("places").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        self.service
            .add_place(AddPlace {
                path: folder,
                friendly_name: None,
                kind,
            })
            .await
            .expect("add the place")
    }
}
