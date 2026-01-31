//! Shared test support: a real SQLite journal, the real Checker and Resolver, a stand-in for
//! GitHub, ordin's own fixture repositories on disk served through `url.<base>.insteadOf`, and an
//! organisation profile read from its cache. A missing fixture fails; nothing skips.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ed25519_dalek::{Signer as _, SigningKey};
use ordin_catalogue::{CatalogueConfig, CatalogueDeps, CatalogueService};
use ordin_checker::Checker;
use ordin_format_plugins::FormatPlugins;
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, InstallerSettings, OrganisationProfileService, ProfileMode, ProfileOptions,
};
use ordin_projects::{
    BranchReviewRules, BranchReviewStatus, BranchWithFiles, GitCredential, GithubAccess,
    GithubError, GithubPerson, OpenPullRequest, PullRequestRef, PullRequestView, RepoFile,
    RepoFolderRef, RepoPermission, ReviewRequest, ReviewVerdict,
};
use ordin_resolver::{ResolverAndCompiler, ResolverConfig};
use ordin_types::encoding::b64url_encode;
use serde_json::json;
use sha2::{Digest, Sha256};

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-catalogue-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- git: fixture repositories reached through `https://github.com/` ----

fn mirror_root() -> &'static PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("ordin-catalogue-mirror-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("the mirror folder");
        root
    })
}

/// Points libgit2's global configuration at a file mapping `https://github.com/` to the mirror,
/// before any repository is opened in this process.
pub fn install_git_mirror() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let config = mirror_root().join("config");
        std::fs::create_dir_all(&config).expect("the config folder");
        // Forward slashes: a backslash in a git config value is an escape, and Windows paths have them.
        let repos = mirror_root()
            .join("repos")
            .display()
            .to_string()
            .replace('\\', "/");
        let url = if repos.starts_with('/') {
            format!("file://{repos}/")
        } else {
            format!("file:///{repos}/")
        };
        std::fs::write(
            config.join(".gitconfig"),
            format!("[url \"{url}\"]\n\tinsteadOf = https://github.com/\n[user]\n\tname = fixture\n\temail = fixture@example.invalid\n"),
        )
        .expect("the git config");
        // SAFETY: called once, before this process opens any repository.
        unsafe {
            git2::opts::set_search_path(git2::ConfigLevel::Global, &config).expect("the search path");
        }
    });
}

/// A name no other test uses, as an owner segment.
pub fn unique_owner() -> String {
    format!("o{}", uuid::Uuid::now_v7().simple())
}

fn repo_dir(owner: &str, name: &str) -> PathBuf {
    mirror_root()
        .join("repos")
        .join(owner)
        .join(format!("{name}.git"))
}

/// Commits `files` as the whole tree of branch `main` of `owner/name` (created on first use) and
/// returns the commit.
pub fn commit_tree(owner: &str, name: &str, files: &BTreeMap<String, Vec<u8>>) -> String {
    commit_tree_on(owner, name, "main", files)
}

pub fn commit_tree_on(
    owner: &str,
    name: &str,
    branch: &str,
    files: &BTreeMap<String, Vec<u8>>,
) -> String {
    install_git_mirror();
    let dir = repo_dir(owner, name);
    let repo = match git2::Repository::open_bare(&dir) {
        Ok(repo) => repo,
        Err(_) => {
            std::fs::create_dir_all(&dir).expect("the repository folder");
            let mut options = git2::RepositoryInitOptions::new();
            options.bare(true).initial_head("main");
            git2::Repository::init_opts(&dir, &options).expect("a bare repository")
        }
    };
    let tree_id = write_tree(
        &repo,
        files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_slice()))
            .collect(),
    );
    let tree = repo.find_tree(tree_id).expect("find the tree");
    let signature =
        git2::Signature::now("fixture", "fixture@example.invalid").expect("a signature");
    let reference = format!("refs/heads/{branch}");
    let parent = repo
        .find_reference(&reference)
        .ok()
        .and_then(|r| r.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    let oid = repo
        .commit(
            Some(&reference),
            &signature,
            &signature,
            "fixture",
            &tree,
            &parents,
        )
        .expect("a commit");
    oid.to_string()
}

/// Writes a tree object for '/'-separated paths.
fn write_tree(repo: &git2::Repository, files: Vec<(&str, &[u8])>) -> git2::Oid {
    let mut builder = repo.treebuilder(None).expect("a tree builder");
    let mut folders: BTreeMap<&str, Vec<(&str, &[u8])>> = BTreeMap::new();
    for (path, bytes) in files {
        match path.split_once('/') {
            Some((folder, rest)) => folders.entry(folder).or_default().push((rest, bytes)),
            None => {
                let blob = repo.blob(bytes).expect("a blob");
                builder.insert(path, blob, 0o100644).expect("insert a file");
            }
        }
    }
    for (folder, inner) in folders {
        let id = write_tree(repo, inner);
        builder
            .insert(folder, id, 0o040000)
            .expect("insert a folder");
    }
    builder.write().expect("write a tree")
}

/// The whole tree of a repository the test already committed.
pub fn tree_of(files: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
    files
        .iter()
        .map(|(p, c)| ((*p).to_owned(), c.as_bytes().to_vec()))
        .collect()
}

// ---- parts written in the ordin format ----

pub fn yaml(kind: &str, id: &str, name: &str, extra_metadata: &str, body: &str) -> String {
    format!(
        "apiVersion: ai.enterprise/v1\nkind: {kind}\nmetadata:\n  id: {id}\n  name: {name}\n  version: 1.0.0\n  owner: legal-ops\n{extra_metadata}{body}"
    )
}

pub fn markdown(kind: &str, id: &str, name: &str, extra_metadata: &str, body: &str) -> String {
    format!(
        "---\napiVersion: ai.enterprise/v1\nkind: {kind}\nmetadata:\n  id: {id}\n  name: {name}\n  version: 1.0.0\n  owner: legal-ops\n{extra_metadata}---\n{body}"
    )
}

pub fn rule_file(slug: &str, extra_metadata: &str) -> (String, String) {
    (
        format!("models/@acme/rules/{slug}.md"),
        markdown(
            "rule",
            &format!("urn:ordin:acme:rules:{slug}"),
            slug,
            extra_metadata,
            "Use short sentences.\n",
        ),
    )
}

// ---- GitHub stand-in ----

/// A stand-in for GitHub with answers the test sets. It holds no token: `git_credentials` answers
/// with the error the test sets, and the fixture repositories on disk ask for none.
pub struct StubGithub {
    pub permission: Mutex<RepoPermission>,
    pub permission_error: Mutex<Option<GithubError>>,
    pub default_branch: Mutex<String>,
    pub verified: Mutex<bool>,
    pub branch_rules: Mutex<Result<BranchReviewRules, GithubError>>,
    pub pull_request: Mutex<Option<PullRequestView>>,
    pub credentials_error: Mutex<GithubError>,
    pub verified_calls: Mutex<u32>,
    pub verified_error: Mutex<Option<GithubError>>,
}

impl StubGithub {
    pub fn new() -> Arc<StubGithub> {
        Arc::new(StubGithub {
            permission: Mutex::new(RepoPermission::Write),
            permission_error: Mutex::new(None),
            default_branch: Mutex::new("main".to_owned()),
            verified: Mutex::new(true),
            branch_rules: Mutex::new(Ok(BranchReviewRules {
                review_required: true,
                code_owner_review_required: false,
                required_approvals: 1,
            })),
            pull_request: Mutex::new(None),
            credentials_error: Mutex::new(GithubError::SignInNeeded),
            verified_calls: Mutex::new(0),
            verified_error: Mutex::new(None),
        })
    }
}

fn unused<T>() -> Result<T, GithubError> {
    Err(GithubError::Other("not used by the catalogue".to_owned()))
}

#[async_trait]
impl GithubAccess for StubGithub {
    async fn git_credentials(&self, _host: &str) -> Result<GitCredential, GithubError> {
        Err(self.credentials_error.lock().unwrap().clone())
    }
    async fn repository_permission(
        &self,
        _host: &str,
        _repository: &str,
    ) -> Result<RepoPermission, GithubError> {
        if let Some(error) = self.permission_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(*self.permission.lock().unwrap())
    }
    async fn default_branch(&self, _host: &str, _repository: &str) -> Result<String, GithubError> {
        Ok(self.default_branch.lock().unwrap().clone())
    }
    async fn commit_verified(
        &self,
        _host: &str,
        _repository: &str,
        _sha: &str,
    ) -> Result<bool, GithubError> {
        *self.verified_calls.lock().unwrap() += 1;
        if let Some(error) = self.verified_error.lock().unwrap().clone() {
            return Err(error);
        }
        Ok(*self.verified.lock().unwrap())
    }
    async fn branch_review_rules(
        &self,
        _host: &str,
        _repository: &str,
        _branch: &str,
    ) -> Result<BranchReviewRules, GithubError> {
        self.branch_rules.lock().unwrap().clone()
    }
    async fn pull_request(
        &self,
        _host: &str,
        _repository: &str,
        _number: u64,
    ) -> Result<PullRequestView, GithubError> {
        self.pull_request
            .lock()
            .unwrap()
            .clone()
            .ok_or(GithubError::NotFound)
    }
    async fn open_pull_request(
        &self,
        _req: OpenPullRequest,
    ) -> Result<PullRequestRef, GithubError> {
        unused()
    }
    async fn pull_request_for_branch(
        &self,
        _host: &str,
        _repository: &str,
        _head: &str,
    ) -> Result<Option<PullRequestRef>, GithubError> {
        unused()
    }
    async fn create_branch_with_files(&self, _req: BranchWithFiles) -> Result<String, GithubError> {
        unused()
    }
    async fn review_requests(&self, _label: &str) -> Result<Vec<ReviewRequest>, GithubError> {
        unused()
    }
    async fn review(
        &self,
        _h: &str,
        _r: &str,
        _n: u64,
        _v: ReviewVerdict,
        _b: &str,
    ) -> Result<(), GithubError> {
        unused()
    }
    async fn merge(&self, _h: &str, _r: &str, _n: u64) -> Result<bool, GithubError> {
        unused()
    }
    async fn close(&self, _h: &str, _r: &str, _n: u64) -> Result<(), GithubError> {
        unused()
    }
    async fn reviewers(
        &self,
        _h: &str,
        _r: &str,
        _q: &str,
    ) -> Result<Vec<GithubPerson>, GithubError> {
        unused()
    }
    async fn file_at(
        &self,
        _h: &str,
        _r: &str,
        _g: &str,
        _p: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        unused()
    }
    async fn update_file(
        &self,
        _h: &str,
        _r: &str,
        _b: &str,
        _p: &str,
        _bytes: Vec<u8>,
        _e: &str,
        _m: &str,
    ) -> Result<String, GithubError> {
        unused()
    }
    async fn list_files(&self, _f: &RepoFolderRef, _g: &str) -> Result<Vec<RepoFile>, GithubError> {
        unused()
    }
    async fn read_file(
        &self,
        _f: &RepoFolderRef,
        _n: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        unused()
    }
    async fn file_last_commit(&self, _f: &RepoFolderRef, _n: &str) -> Result<String, GithubError> {
        unused()
    }
    async fn branch_review_status(
        &self,
        _h: &str,
        _r: &str,
        _b: &str,
    ) -> Result<BranchReviewStatus, GithubError> {
        unused()
    }
}

// ---- the organisation profile, from its cache ----

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> ordin_types::time::Timestamp {
        ordin_types::time::Timestamp::now()
    }
}

/// One repository the profile names.
#[derive(Clone)]
pub struct Entry {
    pub key: String,
    pub display_name: String,
    pub host: String,
    pub repository: String,
    pub branch: String,
    pub format: &'static str,
    pub signed: bool,
}

impl Entry {
    pub fn ordin(key: &str, owner: &str, name: &str) -> Entry {
        Entry {
            key: key.to_owned(),
            display_name: format!("{name} agents"),
            host: "github.com".to_owned(),
            repository: format!("{owner}/{name}"),
            branch: "main".to_owned(),
            format: "ordin",
            signed: false,
        }
    }
}

/// An active, signed profile written the way the service caches one, then read at start.
pub async fn profile_service(
    dir: &Path,
    pool: &sqlx::SqlitePool,
    entries: &[Entry],
    person_may_add: bool,
) -> OrganisationProfileService {
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let public: [u8; 32] = key.verifying_key().to_bytes();
    let kid = {
        let digest: [u8; 32] = Sha256::digest(public).into();
        ordin_types::num::Sha256Hex::from_digest(&digest).0[..16].to_owned()
    };
    let repositories: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            json!({"key": e.key, "display_name": e.display_name, "host": e.host, "repository": e.repository,
                   "protected_branch": e.branch, "format": e.format, "signed_merges_required": e.signed})
        })
        .collect();
    let version = format!("1.0.{}", uuid::Uuid::now_v7().as_u128() % 100_000);
    let document = serde_json::to_vec(&json!({
        "format_version": 1,
        "profile_version": version,
        "agent_catalogue": {"repositories": repositories, "person_may_add": person_may_add}
    }))
    .expect("a profile document");
    let header = b64url_encode(format!(r#"{{"alg":"EdDSA","kid":"{kid}"}}"#).as_bytes());
    let input = format!("{header}.{}", b64url_encode(&document));
    let jws = format!(
        "{header}..{}",
        b64url_encode(&key.sign(input.as_bytes()).to_bytes())
    );
    let cache = dir.join(format!("profile-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&cache).expect("the cache folder");
    std::fs::write(cache.join("active.json"), &document).expect("the cached document");
    std::fs::write(cache.join("active.jws"), &jws).expect("the cached signature");
    let digest: [u8; 32] = Sha256::digest(&document).into();
    let now = ordin_types::time::Timestamp::now().to_wire();
    sqlx::query("UPDATE organisation_profiles SET state = 'replaced' WHERE state = 'active'")
        .execute(pool)
        .await
        .expect("replace the old profile");
    sqlx::query(
        "INSERT INTO organisation_profiles (id, profile_version, signature, document_digest, cached_path, state, \
         fetched_at, verified_at, activated_at) VALUES (?1,?2,?3,?4,?5,'active',?6,?6,?6)",
    )
    .bind(ordin_types::OrganisationProfileId::new().to_string())
    .bind(&version)
    .bind(&jws)
    .bind(ordin_types::num::Sha256Hex::from_digest(&digest).0)
    .bind(cache.to_string_lossy().into_owned())
    .bind(&now)
    .execute(pool)
    .await
    .expect("the cached profile row");
    OrganisationProfileService::start(
        ProfileMode::Read {
            pool: pool.clone(),
            data_root: dir.to_path_buf(),
            http: reqwest::Client::new(),
            build_defaults: InstallerSettings {
                profile_url: None,
                profile_signing_keys: vec![b64url_encode(&public)],
                update_feed_url: None,
            },
        },
        ProfileOptions {
            refresh_interval: Duration::from_secs(3600),
            clock: Arc::new(WallClock),
        },
    )
    .await
    .expect("the profile service")
}

// ---- the whole environment ----

pub struct Env {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub checker: Arc<Checker>,
    pub resolver: Arc<ResolverAndCompiler>,
    pub github: Arc<StubGithub>,
    pub formats: Arc<FormatPlugins>,
    pub profile: Arc<OrganisationProfileService>,
    pub service: CatalogueService,
    pub bundled: PathBuf,
}

impl Env {
    /// A catalogue over a fresh journal, with `entries` in the profile and nothing fetched yet.
    pub async fn new(entries: &[Entry]) -> Env {
        Env::with(entries, true).await
    }

    pub async fn with(entries: &[Entry], person_may_add: bool) -> Env {
        Env::build(entries, person_may_add, true).await
    }

    /// A catalogue whose Checker has no part source bound (the Checker is unavailable to it).
    pub async fn unbound(entries: &[Entry]) -> Env {
        Env::build(entries, true, false).await
    }

    async fn build(entries: &[Entry], person_may_add: bool, bind: bool) -> Env {
        install_git_mirror();
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
            .register(ordin_journal::ModuleMigrations {
                module: ordin_organisation_profile::MODULE_NAME,
                sqlite: ordin_organisation_profile::SQLITE_MIGRATIONS,
                postgres: &[],
            })
            .expect("register the profile");
        registry
            .register(ordin_checker::migrations())
            .expect("register the checker");
        registry
            .register(ordin_catalogue::migrations())
            .expect("register the catalogue");
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
        let resolver = Arc::new(
            ResolverAndCompiler::new(ResolverConfig {
                bundle_root: dir.0.join("bundles"),
                compiler_version: "0.1.0-test".to_owned(),
            })
            .expect("a resolver"),
        );
        let pool = store.sqlite().expect("SQLite").clone();
        let profile = Arc::new(profile_service(&dir.0, &pool, entries, person_may_add).await);
        let checker = Arc::new(Checker::new(
            Arc::clone(&resolver),
            Arc::clone(&profile),
            Arc::clone(&journal),
        ));
        let github = StubGithub::new();
        let formats = Arc::new(FormatPlugins::builtin());
        let bundled = dir.0.join("bundled");
        std::fs::create_dir_all(&bundled).expect("the bundled folder");
        let service = CatalogueService::new(
            CatalogueDeps {
                datastore: store.clone(),
                profile: Arc::clone(&profile),
                github: github.clone(),
                formats: Arc::clone(&formats),
                checker: Arc::clone(&checker),
            },
            CatalogueConfig {
                root: dir.0.join("catalogue"),
                bundled_catalogue: bundled.clone(),
                ordin_version: "0.1.0-test".to_owned(),
            },
        );
        let source: Arc<dyn ordin_agent_model::PartSource> = Arc::new(service.clone());
        if bind {
            checker
                .install_part_source(Arc::clone(&source))
                .expect("bind the checker");
        }
        resolver
            .install_part_source(source)
            .expect("bind the resolver");
        Env {
            dir,
            store,
            journal,
            checker,
            resolver,
            github,
            formats,
            profile,
            service,
            bundled,
        }
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    /// A second catalogue over the same datastore whose profile names `entries` (a restart after the
    /// company changed its profile).
    pub async fn restart_with(&self, entries: &[Entry]) -> CatalogueService {
        let profile = Arc::new(profile_service(&self.dir.0, self.pool(), entries, true).await);
        CatalogueService::new(
            CatalogueDeps {
                datastore: self.store.clone(),
                profile,
                github: self.github.clone(),
                formats: Arc::clone(&self.formats),
                checker: Arc::clone(&self.checker),
            },
            CatalogueConfig {
                root: self.dir.0.join("catalogue"),
                bundled_catalogue: self.bundled.clone(),
                ordin_version: "0.1.0-test".to_owned(),
            },
        )
    }

    pub async fn count(&self, sql: &str) -> i64 {
        sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_owned()))
            .fetch_one(self.pool())
            .await
            .expect("a count")
    }
}

/// The repositories of a profile whose `Entry`s name `owner/name` pairs.
pub fn names(owner: &str, names: &[&str]) -> Vec<Entry> {
    names.iter().map(|n| Entry::ordin(n, owner, n)).collect()
}

/// Every file of a committed fixture folder under `testdata/`, keyed by its path inside it.
pub fn testdata_tree(relative: &str) -> BTreeMap<String, Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(relative);
    assert!(
        root.is_dir(),
        "the fixture folder {} is missing",
        root.display()
    );
    let mut out = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("a fixture folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let key = path
                    .strip_prefix(root)
                    .expect("inside the root")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(key, std::fs::read(&path).expect("a fixture file"));
            }
        }
    }
    walk(&root, &root, &mut out);
    out
}

// ---- a valid ordin-format repository ----

pub const DESCRIPTIVE: &str = "  category: legal\n  audience:\n    roles: [lawyer]\n";
pub const ROLE_ID: &str = "urn:ordin:acme:system-roles:reviewer";
pub const TONE_ID: &str = "urn:ordin:acme:rules:tone";

fn vocabulary_file(slug: &str, axis: &str, terms: &[(&str, &str)]) -> (String, String) {
    let mut body = format!("axis: {axis}\nterms:\n");
    for (code, label) in terms {
        body.push_str(&format!("  - code: {code}\n    label: {label}\n"));
    }
    (
        format!("models/@acme/vocabulary/{slug}.yaml"),
        yaml(
            "vocabulary",
            &format!("urn:ordin:acme:vocabulary:{slug}"),
            slug,
            "",
            &body,
        ),
    )
}

/// The six vocabularies, a system role and the tone rule: the base every valid repository holds.
pub fn base_files() -> BTreeMap<String, Vec<u8>> {
    let mut files: Vec<(String, String)> = vec![
        vocabulary_file("categories", "category", &[("legal", "Legal")]),
        vocabulary_file("roles", "audience-role", &[("lawyer", "Lawyer")]),
        vocabulary_file(
            "pack-classes",
            "pack-class",
            &[
                ("stack", "technology stack"),
                ("testing", "testing approach"),
            ],
        ),
        vocabulary_file(
            "memory-classes",
            "memory-content-class",
            &[
                ("decisions", "Decisions"),
                ("open-questions", "Open questions"),
            ],
        ),
        vocabulary_file(
            "loop-strategies",
            "loop-strategy",
            &[("observe-then-act", "Observe then act")],
        ),
        vocabulary_file(
            "capabilities",
            "runtime-capability",
            &[("evidence", "Evidence")],
        ),
        (
            "models/@acme/system-roles/reviewer.md".to_owned(),
            markdown(
                "system-role",
                ROLE_ID,
                "Reviewer",
                "",
                "# Reviewer\n\nReviews contracts.\n",
            ),
        ),
    ];
    let (path, text) = rule_file("tone", "");
    files.push((path, text));
    files
        .into_iter()
        .map(|(p, t)| (p, t.into_bytes()))
        .collect()
}

/// A WorkspaceProfile with the descriptive metadata the Checker requires.
pub fn agent_file(slug: &str, description: &str) -> (String, Vec<u8>) {
    (
        format!("models/@acme/workspaces/{slug}/workspace-profile.yaml"),
        yaml(
            "workspace-profile",
            &format!("urn:ordin:acme:workspaces:{slug}"),
            slug,
            &format!("  description: {description}\n{DESCRIPTIVE}"),
            &format!("systemRole: {ROLE_ID}\ndefaultRules:\n  - {TONE_ID}\n"),
        )
        .into_bytes(),
    )
}

/// The base repository and one agent per slug.
pub fn agents_repo(slugs: &[&str]) -> BTreeMap<String, Vec<u8>> {
    let mut files = base_files();
    for slug in slugs {
        let (path, bytes) = agent_file(
            slug,
            &format!("{slug} reviews contracts. It writes a summary."),
        );
        files.insert(path, bytes);
    }
    files
}

pub fn with(
    mut files: BTreeMap<String, Vec<u8>>,
    path: &str,
    text: &str,
) -> BTreeMap<String, Vec<u8>> {
    files.insert(path.to_owned(), text.as_bytes().to_vec());
    files
}

/// Sync the profile repositories and return the one outcome of the one repository.
pub async fn sync_one(env: &Env) -> ordin_catalogue::RepositorySyncOutcome {
    let report = env
        .service
        .sync_profile_repositories()
        .await
        .expect("the sync");
    report
        .results
        .into_iter()
        .next()
        .expect("one result")
        .outcome
}

pub async fn last_result(
    env: &Env,
    display_contains: &str,
) -> Option<ordin_catalogue::FetchResult> {
    env.service
        .list_repositories()
        .await
        .expect("the list")
        .into_iter()
        .find(|r| r.repository.display_name.contains(display_contains))
        .and_then(|r| r.last_fetch)
        .map(|f| f.result)
}

pub async fn repo_id(env: &Env, display_contains: &str) -> ordin_types::AgentRepositoryId {
    env.service
        .list_repositories()
        .await
        .expect("the list")
        .into_iter()
        .find(|r| r.repository.display_name.contains(display_contains))
        .expect("the repository")
        .repository
        .id
}

// ---- the PartSource contract over real repositories ----

use ordin_agent_model::testkit::ContractFixture;
use ordin_agent_model::{
    content_digest, markdown_body, read_tree, render_part, Manifest, ModelPart, PartFiles,
    PartKind, RepositoryRef, VocabularySet, YamlValue,
};

/// A part with its metadata and body edited, re-rendered and re-digested.
pub fn rebuild(model: &ModelPart, edit: impl FnOnce(&mut Manifest)) -> ModelPart {
    let mut manifest = model.manifest.clone();
    edit(&mut manifest);
    let name = model
        .manifest_path
        .rsplit('/')
        .next()
        .expect("a file name")
        .to_owned();
    let text = model
        .files
        .0
        .get(&name)
        .and_then(|bytes| markdown_body(&model.manifest_path, bytes));
    let rendered = render_part(model.kind, &manifest, text.as_deref()).expect("a rendered part");
    let files = PartFiles(BTreeMap::from([(name, rendered.content)]));
    ModelPart {
        kind: model.kind,
        content_digest: content_digest(&files),
        manifest,
        manifest_path: model.manifest_path.clone(),
        files,
    }
}

/// One part from one file of the ordin format.
pub fn part_from_text(path: &str, text: &str) -> ModelPart {
    let files = BTreeMap::from([(path.to_owned(), text.as_bytes().to_vec())]);
    let reading = read_tree(&files, &VocabularySet::default());
    assert!(reading.refused.is_empty(), "refused: {:?}", reading.refused);
    reading.parts.into_iter().next().expect("one part")
}

fn str_seq(items: &[&str]) -> YamlValue {
    YamlValue::Seq(
        items
            .iter()
            .map(|s| YamlValue::Str((*s).to_owned()))
            .collect(),
    )
}

/// The vocabulary parts every repository of the contract needs, as `ModelPart`s.
pub fn vocabulary_parts(publisher_slug: &str, category_terms: &[(&str, &str)]) -> Vec<ModelPart> {
    let one = |slug: &str, axis: &str, terms: &[(&str, &str)]| {
        let (path, text) = {
            let mut body = format!("axis: {axis}\nterms:\n");
            for (code, label) in terms {
                body.push_str(&format!("  - code: {code}\n    label: {label}\n"));
            }
            (
                format!("models/@acme/vocabulary/{publisher_slug}-{slug}.yaml"),
                yaml(
                    "vocabulary",
                    &format!("urn:ordin:acme:vocabulary:{publisher_slug}-{slug}"),
                    slug,
                    "",
                    &body,
                ),
            )
        };
        part_from_text(&path, &text)
    };
    vec![
        one("categories", "category", category_terms),
        one("roles", "audience-role", &[("lawyer", "Lawyer")]),
        one(
            "pack-classes",
            "pack-class",
            &[("stack", "technology stack")],
        ),
        one(
            "memory-classes",
            "memory-content-class",
            &[("decisions", "Decisions")],
        ),
        one(
            "loop-strategies",
            "loop-strategy",
            &[("observe-then-act", "Observe then act")],
        ),
        one(
            "capabilities",
            "runtime-capability",
            &[("evidence", "Evidence")],
        ),
    ]
}

/// `ContractFixture::standard()` with the descriptive fields the real Checker requires added, so
/// that every part the contract needs offered passes its checks (CatalogueService.md §L12: the
/// repositories are "equivalent to" the standard fixture). Identifiers, versions, statuses and the
/// resolution structure are unchanged.
pub fn contract_fixture() -> ContractFixture {
    let mut fixture = ContractFixture::standard();
    let role = part_from_text(
        "models/@acme/system-roles/reviewer.md",
        &markdown(
            "system-role",
            ROLE_ID,
            "Reviewer",
            "",
            "# Reviewer\n\nReviews contracts.\n",
        ),
    );
    let describe = |m: &mut Manifest| {
        m.metadata.category = "legal".to_owned();
        m.metadata.audience.roles = vec!["lawyer".to_owned()];
        m.metadata.description = "Does the work. It writes a summary.".to_owned();
    };
    for (_, label, parts) in fixture.repositories.iter_mut() {
        let home = label.ends_with("/home");
        let bundled = label == "bundled";
        for part in parts.iter_mut() {
            if part.kind == PartKind::WorkspaceProfile {
                *part = rebuild(part, |m| {
                    describe(m);
                    m.body
                        .insert("systemRole".to_owned(), YamlValue::Str(ROLE_ID.to_owned()));
                });
            }
            if part.kind == PartKind::PracticePack {
                *part = rebuild(part, |m| {
                    describe(m);
                    m.metadata.class = "stack".to_owned();
                });
            }
        }
        if home {
            parts.retain(|p| p.kind != PartKind::Vocabulary);
            parts.extend(vocabulary_parts(
                "home",
                &[("finance", "Finance"), ("legal", "Legal")],
            ));
            parts.push(role.clone());
        }
        if bundled {
            parts.extend(vocabulary_parts("bundled", &[("legal", "Legal")]));
        }
    }
    fixture
}

/// Writes the parts of one fixture repository as a tree: every part in a folder of its own, because
/// several versions of one identifier share one manifest path.
pub fn tree_of_parts(parts: &[ModelPart], extra: &[(&str, String)]) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for (at, part) in parts.iter().enumerate() {
        let publisher = part
            .manifest_path
            .split('/')
            .nth(1)
            .unwrap_or("@acme")
            .to_owned();
        for (name, bytes) in &part.files.0 {
            files.insert(
                format!("models/{publisher}/fixture/p{at}/{name}"),
                bytes.clone(),
            );
        }
    }
    for (path, text) in extra {
        files.insert((*path).to_owned(), text.clone().into_bytes());
    }
    files
}

pub fn repository_row_id(reference: &RepositoryRef) -> ordin_types::AgentRepositoryId {
    match reference {
        RepositoryRef::Repository(id) => *id,
        RepositoryRef::Bundled => ordin_agent_model::BUNDLED_REPOSITORY_ID,
    }
}

impl Env {
    /// A repository row with a given id, as the program's profile or the person would have made it.
    pub async fn insert_repository(
        &self,
        id: ordin_types::AgentRepositoryId,
        label: &str,
        source: &str,
    ) {
        let (host, repository) = label.split_once('/').expect("host/owner/name");
        let clone = self
            .dir
            .0
            .join("catalogue")
            .join("clones")
            .join(host)
            .join(repository);
        let signed = if source == "profile" { "0" } else { "NULL" };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO agent_repositories (id, display_name, host, repository, protected_branch, format, format_version, \
             source, signed_merges_required, person_can_publish, person_can_curate, last_fetched_commit, profile_key, \
             clone_path, state, added_at) VALUES (?1, ?2, ?3, ?4, 'main', 'ordin', 'unknown', '{source}', {signed}, 0, 0, NULL, NULL, ?5, 'named', ?6)"
        )))
        .bind(id.to_string())
        .bind(label)
        .bind(host)
        .bind(repository)
        .bind(clone.to_string_lossy().into_owned())
        .bind(ordin_types::time::Timestamp::now().to_wire())
        .execute(self.pool())
        .await
        .expect("a repository row");
    }
}

pub fn withdrawn_text(kind: &str, id: &str) -> String {
    format!(
        "listSchema: ordin.withdrawn/v1\nentries:\n  - part: {{ kind: {kind}, id: \"{id}\" }}\n    reason: \"Replaced.\"\n    withdrawnAt: \"2026-01-10T09:00:00.000Z\"\n"
    )
}
