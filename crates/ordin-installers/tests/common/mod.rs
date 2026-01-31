//! Shared test support: a scratch directory, the committed fixtures of
//! `testdata/parity/installers/`, a resolver over their catalogue, and an Agent Folder Manager on a
//! migrated SQLite journal. Every test writes under its own temporary folder only.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_agent_folders::{AgentFolder, AgentFolderManager};
use ordin_agent_model::testkit::{ContractFixture, InMemorySource};
use ordin_agent_model::{read_tree, PartSource, RepositoryRef, SourceOverride, VocabularySet};
use ordin_installers::*;
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_resolver::{ResolveRequest, ResolverAndCompiler, ResolverConfig};
use ordin_types::AgentInstanceId;

pub const CSPA: &str = "urn:ordin:default:workspaces:cspa-431";
pub const HELPDESK: &str = "urn:ordin:default:workspaces:helpdesk";
pub const TEST_PYRAMID: &str = "urn:ordin:default:practice-packs:testing:test-pyramid";
pub const APPLY: &str = "urn:ordin:default:commands:lifecycle:apply";

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-inst-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(std::fs::canonicalize(dir).expect("a canonical temporary folder"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        restore_modes(&self.0);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Gives back write permission so the clean-up can delete what a test made read-only.
#[cfg(unix)]
pub fn restore_modes(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.is_dir() {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    restore_modes(&entry.path());
                }
            }
        }
    }
}

#[cfg(not(unix))]
pub fn restore_modes(_: &Path) {}

/// `testdata/parity/installers/`.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/parity/installers")
}

/// Every file under `dir`, keyed by its '/'-separated relative path. A folder that cannot be read
/// fails the test; it never skips.
pub fn read_dir_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{} cannot be read: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("inside the base")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(relative, std::fs::read(&path).expect("a readable file"));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// The committed catalogue as an in-memory bundled repository.
pub fn catalogue_source() -> InMemorySource {
    let files = read_dir_files(&fixtures().join("catalogue"));
    let reading = read_tree(&files, &VocabularySet::default());
    assert!(reading.refused.is_empty(), "refused: {:?}", reading.refused);
    InMemorySource::new(&ContractFixture {
        repositories: vec![(RepositoryRef::Bundled, "bundled".to_owned(), reading.parts)],
        withdrawn: Vec::new(),
        refused: Vec::new(),
    })
}

/// The committed catalogue with some files replaced or added (catalogue-relative path → text).
pub fn catalogue_source_with(changes: &[(&str, String)]) -> InMemorySource {
    let mut files = read_dir_files(&fixtures().join("catalogue"));
    for (path, text) in changes {
        files.insert((*path).to_owned(), text.clone().into_bytes());
    }
    let reading = read_tree(&files, &VocabularySet::default());
    assert!(reading.refused.is_empty(), "refused: {:?}", reading.refused);
    InMemorySource::new(&ContractFixture {
        repositories: vec![(RepositoryRef::Bundled, "bundled".to_owned(), reading.parts)],
        withdrawn: Vec::new(),
        refused: Vec::new(),
    })
}

/// The catalogue with a rule `.claude/rules/a.md` that imports itself, added to cspa-431's rules.
pub fn circular_catalogue() -> InMemorySource {
    let profile_path = "models/@default/workspaces/cspa-431/workspace-profile.yaml";
    let profile = text(
        &read_dir_files(&fixtures().join("catalogue"))
            .remove(profile_path)
            .expect("the cspa-431 profile"),
    );
    let profile = profile.replace(
        "  - urn:ordin:default:rules:tone\n",
        "  - urn:ordin:default:rules:tone\n  - urn:ordin:default:rules:a\n",
    );
    let rule = "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:default:rules:a\n  name: A\n  version: 1.0.0\n  owner: engineering-enablement\n---\nFirst line.\n\n@.claude/rules/a.md\n";
    catalogue_source_with(&[
        (profile_path, profile),
        ("models/@default/rules/a.md", rule.to_owned()),
    ])
}

/// A source with no part at all.
pub fn empty_source() -> InMemorySource {
    InMemorySource::new(&ContractFixture {
        repositories: vec![(RepositoryRef::Bundled, "bundled".to_owned(), Vec::new())],
        withdrawn: Vec::new(),
        refused: Vec::new(),
    })
}

pub fn scope() -> SourceScope {
    SourceScope {
        home: RepositoryRef::Bundled,
        profile: Vec::new(),
        include_bundled: true,
    }
}

pub fn resolve_request(root: &str, harness: HarnessKind, packs: &[&str]) -> ResolveRequest {
    ResolveRequest {
        scope: scope(),
        root_id: root.to_owned(),
        root_version: None,
        target_harness: harness,
        workspace_languages: Vec::new(),
        interface_language: "en-GB".to_owned(),
        pack_answers: Vec::new(),
        pack_changes: packs.iter().map(|p| (*p).to_owned()).collect(),
        overlay_changes: Vec::new(),
        replacements: Vec::new(),
        staged: Vec::new(),
        allow_draft_root: false,
        source: None,
    }
}

/// A resolver, a folder manager and the installers over them, in one temporary folder.
pub struct World {
    pub dir: TempDir,
    pub resolver: Arc<ResolverAndCompiler>,
    pub folders: Arc<AgentFolderManager>,
    pub installers: Installers,
    /// When set, every bundle is built from this source while the resolver stays bound to another.
    pub build_source: Option<Arc<dyn PartSource>>,
}

impl World {
    /// The resolver bound to the committed catalogue.
    pub async fn new() -> World {
        World::bound_to(Arc::new(catalogue_source()), None).await
    }

    /// The resolver bound to `bound`; bundles built from `build` when given.
    pub async fn bound_to(bound: Arc<dyn PartSource>, build: Option<Arc<dyn PartSource>>) -> World {
        let dir = TempDir::new();
        let resolver = ResolverAndCompiler::new(ResolverConfig {
            bundle_root: dir.path().join("bundles"),
            compiler_version: "0.1.0-test".to_owned(),
        })
        .expect("a resolver");
        resolver
            .install_part_source(bound)
            .expect("the first binding");
        let resolver = Arc::new(resolver);
        let folders = Arc::new(folder_manager(dir.path()).await);
        let installers = Installers::new(Arc::clone(&folders), Arc::clone(&resolver));
        World {
            dir,
            resolver,
            folders,
            installers,
            build_source: build,
        }
    }

    pub async fn bundle(&self, root: &str, harness: HarnessKind, packs: &[&str]) -> Bundle {
        let mut request = resolve_request(root, harness, packs);
        request.source = self.build_source.clone().map(SourceOverride);
        self.resolver
            .build(&request)
            .await
            .expect("the bundle builds")
    }

    pub async fn agent(&self) -> AgentFolder {
        self.folders
            .create(AgentInstanceId::new())
            .await
            .expect("an agent folder")
    }

    pub fn request(
        &self,
        agent: &AgentFolder,
        harness: HarnessKind,
        bundle: &Bundle,
    ) -> InstallRequest {
        InstallRequest {
            agent_instance_id: agent.agent_instance_id,
            harness,
            plan_hash: bundle.plan.plan_hash.clone(),
        }
    }

    /// An agent with cspa-431 installed for Claude Code.
    pub async fn installed_cspa(&self) -> (AgentFolder, Bundle, InstallOutcome) {
        let bundle = self.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
        let agent = self.agent().await;
        let outcome = self
            .installers
            .install(&self.request(&agent, HarnessKind::ClaudeCode, &bundle))
            .await
            .expect("installs");
        (agent, bundle, outcome)
    }
}

/// An Agent Folder Manager on a migrated, started SQLite journal.
pub async fn folder_manager(dir: &Path) -> AgentFolderManager {
    let data = dir.join("journal");
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: data.join("journal.db"),
        backup_dir: data.join("backups"),
    })
    .await
    .expect("the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal migrations");
    registry
        .register(ordin_agent_folders::migrations())
        .expect("folder migrations");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("migrations apply");
    let journal = Journal::start(
        store,
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("the journal starts");
    let root = dir.join("data");
    std::fs::create_dir_all(&root).expect("the data root");
    AgentFolderManager::new(journal, root)
        .await
        .expect("the folder manager")
}

/// Every file of a folder.
pub fn tree_of(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    read_dir_files(dir)
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8")
}
