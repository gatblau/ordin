//! Shared test helpers: parts written in the ordin format, an in-memory source, a migrated SQLite
//! journal and a Checker whose trust policy the test sets. A missing fixture fails; nothing skips.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_agent_model::testkit::{ContractFixture, InMemorySource};
use ordin_agent_model::{
    read_tree, ModelPart, PartFiles, PartKind, PartSource, PartSourceError, RepositoryRef,
    Resolution, SourceScope, SourcedPart, StagedPart, VocabularyAxis, VocabularySet,
    WithdrawnListEntry,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_resolver::{ResolverAndCompiler, ResolverConfig};
use ordin_types::AgentRepositoryId;

use crate::checker::TrustSource;
use crate::model::{Finding, RepositoryCheck};
use crate::Checker;

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-checker-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The trust policy a test sets and changes.
#[derive(Default)]
pub struct FixedTrust(pub Mutex<Option<serde_json::Value>>);

impl FixedTrust {
    pub fn set(&self, value: serde_json::Value) {
        *self.0.lock().unwrap() = Some(value);
    }
}

impl TrustSource for FixedTrust {
    fn trust_policy(&self) -> (Option<serde_json::Value>, Option<String>) {
        (self.0.lock().unwrap().clone(), Some("7".to_owned()))
    }
}

/// The home repository of the tests.
pub fn acme() -> RepositoryRef {
    RepositoryRef::Repository(AgentRepositoryId(uuid::uuid!(
        "01900000-0000-7000-8000-0000000000a1"
    )))
}

pub const ACME_LABEL: &str = "github.com/acme/agents";

pub fn other() -> RepositoryRef {
    RepositoryRef::Repository(AgentRepositoryId(uuid::uuid!(
        "01900000-0000-7000-8000-0000000000b2"
    )))
}

pub fn scope() -> SourceScope {
    SourceScope {
        home: acme(),
        profile: vec![other()],
        include_bundled: false,
    }
}

/// One part from one file of the ordin format.
pub fn part(path: &str, text: &str) -> ModelPart {
    let mut files = BTreeMap::new();
    files.insert(path.to_owned(), text.as_bytes().to_vec());
    let reading = read_tree(&files, &VocabularySet::default());
    assert!(reading.refused.is_empty(), "refused: {:?}", reading.refused);
    reading.parts.into_iter().next().expect("one part")
}

pub fn staged(part: ModelPart) -> StagedPart {
    StagedPart {
        part,
        repository: acme(),
        repository_label: ACME_LABEL.to_owned(),
    }
}

/// A YAML manifest with the metadata every part needs.
pub fn yaml(kind: &str, id: &str, name: &str, extra_metadata: &str, body: &str) -> String {
    format!(
        "apiVersion: ai.enterprise/v1\nkind: {kind}\nmetadata:\n  id: {id}\n  name: {name}\n  version: 1.0.0\n  owner: legal-ops\n{extra_metadata}{body}"
    )
}

/// A Markdown part.
pub fn markdown(kind: &str, id: &str, name: &str, body: &str) -> String {
    format!(
        "---\napiVersion: ai.enterprise/v1\nkind: {kind}\nmetadata:\n  id: {id}\n  name: {name}\n  version: 1.0.0\n  owner: legal-ops\n---\n{body}"
    )
}

pub fn vocabulary_part(slug: &str, axis: &str, terms: &[(&str, &str)]) -> ModelPart {
    let mut body = format!("axis: {axis}\nterms:\n");
    for (code, label) in terms {
        body.push_str(&format!("  - code: {code}\n    label: {label}\n"));
    }
    part(
        &format!("models/@acme/vocabulary/{slug}.yaml"),
        &yaml(
            "vocabulary",
            &format!("urn:ordin:acme:vocabulary:{slug}"),
            slug,
            "",
            &body,
        ),
    )
}

/// The vocabulary every clean fixture uses.
pub fn vocabularies() -> Vec<ModelPart> {
    vec![
        vocabulary_part("categories", "category", &[("legal", "Legal")]),
        vocabulary_part("roles", "audience-role", &[("lawyer", "Lawyer")]),
        vocabulary_part(
            "pack-classes",
            "pack-class",
            &[
                ("stack", "technology stack"),
                ("testing", "testing approach"),
            ],
        ),
        vocabulary_part(
            "memory-classes",
            "memory-content-class",
            &[
                ("decisions", "Decisions"),
                ("open-questions", "Open questions"),
            ],
        ),
        vocabulary_part(
            "loop-strategies",
            "loop-strategy",
            &[("observe-then-act", "Observe then act")],
        ),
        vocabulary_part(
            "capabilities",
            "runtime-capability",
            &[("evidence", "Evidence")],
        ),
    ]
}

pub fn vocabulary_set() -> VocabularySet {
    let parts = vocabularies();
    let refs: Vec<&ModelPart> = parts.iter().collect();
    VocabularySet::from_parts(&refs).expect("a vocabulary set")
}

pub const DESCRIPTIVE: &str = "  category: legal\n  audience:\n    roles: [lawyer]\n";

pub fn workspace(slug: &str, body: &str) -> ModelPart {
    part(
        &format!("models/@acme/workspaces/{slug}/workspace-profile.yaml"),
        &yaml(
            "workspace-profile",
            &format!("urn:ordin:acme:workspaces:{slug}"),
            slug,
            DESCRIPTIVE,
            body,
        ),
    )
}

pub fn role() -> ModelPart {
    part(
        "models/@acme/system-roles/reviewer.md",
        &markdown(
            "system-role",
            "urn:ordin:acme:system-roles:reviewer",
            "Reviewer",
            "# Reviewer\n\nReviews contracts.\n",
        ),
    )
}

pub fn rule(slug: &str, body: &str) -> ModelPart {
    part(
        &format!("models/@acme/rules/{slug}.md"),
        &markdown("rule", &format!("urn:ordin:acme:rules:{slug}"), slug, body),
    )
}

pub fn tool_binding(slug: &str, bindings: &str) -> ModelPart {
    part(
        &format!("models/@acme/tool-bindings/{slug}.yaml"),
        &yaml(
            "tool-binding",
            &format!("urn:ordin:acme:tool-bindings:{slug}"),
            slug,
            "",
            &format!("bindings:\n{bindings}"),
        ),
    )
}

pub fn mcp(name: &str) -> String {
    format!("  - name: {name}\n    permission: read\n    serverType: stdio\n    transport: stdio\n    type: mcp\n")
}

pub const ROLE_ID: &str = "urn:ordin:acme:system-roles:reviewer";
pub const TONE_ID: &str = "urn:ordin:acme:rules:tone";

/// The body of a minimal agent: a role and one rule.
pub fn minimal_body() -> String {
    format!("systemRole: {ROLE_ID}\ndefaultRules:\n  - {TONE_ID}\n")
}

/// The clean base of every repository: the vocabularies, a role and the tone rule.
pub fn base() -> Vec<ModelPart> {
    let mut parts = vocabularies();
    parts.push(role());
    parts.push(rule("tone", "Use short sentences.\n"));
    parts
}

pub fn source_of(home: Vec<ModelPart>, withdrawn: Vec<WithdrawnListEntry>) -> Arc<InMemorySource> {
    source_with(home, Vec::new(), withdrawn)
}

pub fn source_with(
    home: Vec<ModelPart>,
    elsewhere: Vec<ModelPart>,
    withdrawn: Vec<WithdrawnListEntry>,
) -> Arc<InMemorySource> {
    Arc::new(InMemorySource::new(&ContractFixture {
        repositories: vec![
            (acme(), ACME_LABEL.to_owned(), home),
            (other(), "github.com/other/agents".to_owned(), elsewhere),
        ],
        withdrawn,
        refused: Vec::new(),
    }))
}

pub fn repository_check(changes: Vec<StagedPart>) -> RepositoryCheck {
    RepositoryCheck {
        repository: acme(),
        repository_display_name: "Acme agents".to_owned(),
        scope: scope(),
        changes,
        deletions: Vec::new(),
        source: None,
    }
}

pub struct Fixture {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub resolver: Arc<ResolverAndCompiler>,
    pub trust: Arc<FixedTrust>,
    pub checker: Checker,
}

impl Fixture {
    /// The Checker with no source bound.
    pub async fn unbound() -> Fixture {
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
            .expect("register the checker");
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
        let trust = Arc::new(FixedTrust::default());
        let checker = Checker::with_trust_source(
            Arc::clone(&resolver),
            Arc::clone(&trust) as Arc<dyn TrustSource>,
            Arc::clone(&journal),
        );
        Fixture {
            dir,
            store,
            journal,
            resolver,
            trust,
            checker,
        }
    }

    /// The Checker and the Resolver bound to `source`.
    pub async fn with(source: Arc<dyn PartSource>) -> Fixture {
        let f = Fixture::unbound().await;
        f.resolver
            .install_part_source(Arc::clone(&source))
            .expect("bind the resolver");
        f.checker
            .install_part_source(source)
            .expect("bind the checker");
        f
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub async fn runs(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM check_runs")
            .fetch_one(self.pool())
            .await
            .expect("count runs")
    }

    pub async fn journal_kinds(&self) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT kind FROM journal_entries WHERE component = 'checker' ORDER BY recorded_at",
        )
        .fetch_all(self.pool())
        .await
        .expect("journal entries")
    }

    pub fn db_path(&self) -> PathBuf {
        self.dir.0.join("data").join("journal.db")
    }
}

pub fn codes(findings: &[Finding]) -> Vec<&str> {
    findings.iter().map(|f| f.code.as_str()).collect()
}

pub fn has(findings: &[Finding], code: &str) -> bool {
    findings.iter().any(|f| f.code == code)
}

pub fn named<'a>(findings: &'a [Finding], code: &str) -> Vec<&'a Finding> {
    findings.iter().filter(|f| f.code == code).collect()
}

/// Which source call fails.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fails {
    RepositoryParts,
    Withdrawn,
    Resolve,
}

/// A source that delegates to `inner` and fails one call with `error`.
pub struct FailingSource {
    pub inner: Arc<dyn PartSource>,
    pub fails: Fails,
    pub error: PartSourceError,
}

#[async_trait::async_trait]
impl PartSource for FailingSource {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        if self.fails == Fails::Resolve {
            return Err(self.error.clone());
        }
        self.inner.resolve(scope, kind, id, version).await
    }
    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        self.inner.resolve_any_kind(scope, id).await
    }
    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        self.inner.files(part).await
    }
    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.ingredients(scope).await
    }
    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        if self.fails == Fails::Withdrawn {
            return Err(self.error.clone());
        }
        self.inner.withdrawn(scope).await
    }
    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        if self.fails == Fails::RepositoryParts {
            return Err(self.error.clone());
        }
        self.inner.repository_parts(repository).await
    }
    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        self.inner.vocabulary(scope).await
    }
    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.used_by(part).await
    }
    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.citing_parts(repository, axis, code).await
    }
}

/// The workspace root of the repository.
pub fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root")
}
