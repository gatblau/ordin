//! Shared test support: a catalogue builder over the Agent Model's tree reader, an in-memory source
//! with injectable faults, request defaults and a scratch directory. Each integration test binary
//! includes this module and uses part of it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordin_agent_model::testkit::{ContractFixture, InMemorySource};
use ordin_agent_model::{
    read_tree, PartFiles, PartKind, PartSource, PartSourceError, RepositoryRef, Resolution,
    SourceScope, SourcedPart, VocabularySet, WithdrawnListEntry, BUNDLED_REPOSITORY_ID,
};
use ordin_resolver::{ResolveRequest, ResolverAndCompiler, ResolverConfig};
use ordin_types::domain::HarnessKind;

/// A scratch directory removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let path =
            std::env::temp_dir().join(format!("ordin-resolver-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        TempDir(path)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The identifiers of the standard catalogue.
pub const ROOT: &str = "urn:ordin:t:workspaces:agent";
pub const ROLE: &str = "urn:ordin:t:system-roles:agent-role";
pub const RULE_A: &str = "urn:ordin:t:rules:alpha";
pub const RULE_B: &str = "urn:ordin:t:rules:beta";
pub const SKILL: &str = "urn:ordin:t:skills:craft";
pub const CAP: &str = "urn:ordin:t:capabilities:work";
pub const BINDING: &str = "urn:ordin:t:tool-bindings:ops";
pub const GO: &str = "urn:ordin:t:practice-packs:stack:go";
pub const RUST: &str = "urn:ordin:t:practice-packs:stack:rust";
pub const BDD: &str = "urn:ordin:t:practice-packs:testing:bdd";
pub const PYRAMID: &str = "urn:ordin:t:practice-packs:testing:test-pyramid";
pub const TROPHY: &str = "urn:ordin:t:practice-packs:testing:test-trophy";
pub const HONEYCOMB: &str = "urn:ordin:t:practice-packs:testing:testing-honeycomb";
pub const EN_GB: &str = "urn:ordin:t:overlays:language:en-GB";
pub const EN_US: &str = "urn:ordin:t:overlays:language:en-US";
pub const EN: &str = "urn:ordin:t:overlays:language:en";
pub const FR: &str = "urn:ordin:t:overlays:language:fr";
pub const ES: &str = "urn:ordin:t:overlays:language:es";
pub const PLAIN: &str = "urn:ordin:t:overlays:style:plain";

/// A repository tree under construction.
#[derive(Clone, Default)]
pub struct Cat {
    pub files: BTreeMap<String, Vec<u8>>,
}

fn slug(id: &str) -> String {
    id.rsplit(':').next().unwrap_or(id).to_owned()
}

impl Cat {
    pub fn new() -> Cat {
        Cat::default()
    }

    fn header(kind: &str, id: &str, version: &str, meta: &str) -> String {
        format!(
            "apiVersion: ai.enterprise/v1\nkind: {kind}\nmetadata:\n  id: {id}\n  name: {}\n  version: {version}\n  owner: acme\n{meta}",
            slug(id)
        )
    }

    /// A YAML-authored part. `meta` is extra `metadata:` lines (two-space indent), `body` top-level lines.
    pub fn yaml(
        &mut self,
        kind: &str,
        id: &str,
        version: &str,
        meta: &str,
        body: &str,
    ) -> &mut Cat {
        let path = format!("models/{kind}/{}-{version}.yaml", id.replace(':', "_"));
        let text = format!("{}{body}", Cat::header(kind, id, version, meta));
        self.files.insert(path, text.into_bytes());
        self
    }

    /// A Markdown-authored part: front matter, then `text`.
    pub fn md(
        &mut self,
        kind: &str,
        id: &str,
        version: &str,
        meta: &str,
        body: &str,
        text: &str,
    ) -> &mut Cat {
        let path = format!("models/{kind}/{}-{version}.md", id.replace(':', "_"));
        let content = format!(
            "---\n{}{body}---\n{text}\n",
            Cat::header(kind, id, version, meta)
        );
        self.files.insert(path, content.into_bytes());
        self
    }

    pub fn rule(&mut self, id: &str, text: &str) -> &mut Cat {
        self.md("rule", id, "1.0.0", "", "", text)
    }

    pub fn skill(&mut self, id: &str, version: &str, meta: &str) -> &mut Cat {
        self.md("skill", id, version, meta, "", "How to craft.")
    }

    pub fn pack(&mut self, id: &str, class: &str, languages: &[&str], body: &str) -> &mut Cat {
        let mut meta = format!("  class: {class}\n");
        if !languages.is_empty() {
            meta.push_str("  audience:\n    languages:\n");
            for language in languages {
                meta.push_str(&format!("      - {language}\n"));
            }
        }
        self.yaml("practice-pack", id, "1.0.0", &meta, body)
    }

    pub fn overlay(&mut self, kind: &str, id: &str, text: &str) -> &mut Cat {
        self.md(kind, id, "1.0.0", "", "", text)
    }

    pub fn raw(&mut self, path: &str, bytes: &[u8]) -> &mut Cat {
        self.files.insert(path.to_owned(), bytes.to_vec());
        self
    }

    /// The root plus the constituents every simple test needs. `root_body` adds top-level lines.
    pub fn basic(root_body: &str) -> Cat {
        let mut cat = Cat::new();
        cat.yaml(
            "workspace-profile",
            ROOT,
            "1.0.0",
            "",
            &format!(
                "systemRole: {ROLE}\ndefaultRules:\n  - {RULE_A}\n  - {RULE_B}\ncapabilities:\n  - {CAP}\n{root_body}"
            ),
        );
        cat.md("system-role", ROLE, "1.0.0", "", "", "You build things.");
        cat.rule(RULE_A, "Rule alpha text.");
        cat.rule(RULE_B, "Rule beta text.");
        cat.skill(SKILL, "1.0.0", "");
        cat.yaml(
            "capability",
            CAP,
            "1.0.0",
            "",
            &format!("skills:\n  - {SKILL}\n"),
        );
        cat
    }

    /// The four stack and testing packs, and the language overlays.
    pub fn with_packs(&mut self) -> &mut Cat {
        self.pack(GO, "stack", &["go"], "");
        self.pack(RUST, "stack", &["rust"], "");
        for id in [BDD, PYRAMID, TROPHY, HONEYCOMB] {
            self.pack(id, "testing", &[], "");
        }
        self
    }

    pub fn with_languages(&mut self) -> &mut Cat {
        for id in [EN_GB, EN_US, EN, FR, ES] {
            self.overlay("language-overlay", id, "Language text.");
        }
        self
    }

    pub fn parts(&self) -> Vec<ordin_agent_model::ModelPart> {
        let reading = read_tree(&self.files, &VocabularySet::default());
        assert!(reading.refused.is_empty(), "refused: {:?}", reading.refused);
        reading.parts
    }
}

pub fn scope() -> SourceScope {
    SourceScope {
        home: RepositoryRef::Bundled,
        profile: Vec::new(),
        include_bundled: true,
    }
}

pub fn withdrawal(kind: PartKind, id: &str, replacement: Option<&str>) -> WithdrawnListEntry {
    use ordin_agent_model::Timestamp;
    use ordin_types::WithdrawnListEntryId;
    WithdrawnListEntry {
        id: WithdrawnListEntryId::new(),
        agent_repository_id: BUNDLED_REPOSITORY_ID,
        part_kind: kind,
        part_identifier: id.to_owned(),
        part_version: None,
        part_content_digest: None,
        reason: "Replaced.".to_owned(),
        withdrawn_at: Timestamp::from_unix_millis(1_760_000_000_000).expect("a timestamp"),
        replacement_identifier: replacement.map(str::to_owned),
        replacement_version: None,
        removed_by_hand_seen_at: None,
        read_at: Timestamp::from_unix_millis(1_760_000_000_000).expect("a timestamp"),
    }
}

/// The in-memory source over one catalogue held as the bundled repository.
pub fn memory(cat: &Cat, withdrawn: Vec<WithdrawnListEntry>) -> InMemorySource {
    InMemorySource::new(&ContractFixture {
        repositories: vec![(RepositoryRef::Bundled, "bundled".to_owned(), cat.parts())],
        withdrawn,
        refused: Vec::new(),
    })
}

pub type Tweak = Arc<dyn Fn(&mut SourcedPart) + Send + Sync>;

/// A source that delegates to the in-memory one and injects the faults a test asks for.
pub struct FaultySource {
    pub inner: InMemorySource,
    pub tweak: Option<Tweak>,
    pub resolve_error: Option<PartSourceError>,
    pub withdrawn_error: Option<PartSourceError>,
    pub files_error: Option<PartSourceError>,
    /// Append a byte to every file the source returns.
    pub tamper_files: bool,
}

impl FaultySource {
    pub fn new(inner: InMemorySource) -> FaultySource {
        FaultySource {
            inner,
            tweak: None,
            resolve_error: None,
            withdrawn_error: None,
            files_error: None,
            tamper_files: false,
        }
    }

    fn adjust(&self, resolution: Resolution) -> Resolution {
        match (resolution, &self.tweak) {
            (Resolution::Found(mut part), Some(tweak)) => {
                tweak(&mut part);
                Resolution::Found(part)
            }
            (other, _) => other,
        }
    }
}

#[async_trait::async_trait]
impl PartSource for FaultySource {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        if let Some(error) = &self.resolve_error {
            return Err(error.clone());
        }
        let found = self.inner.resolve(scope, kind, id, version).await?;
        Ok(self.adjust(found))
    }

    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        if let Some(error) = &self.resolve_error {
            return Err(error.clone());
        }
        let found = self.inner.resolve_any_kind(scope, id).await?;
        Ok(self.adjust(found))
    }

    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        if let Some(error) = &self.files_error {
            return Err(error.clone());
        }
        let mut files = self.inner.files(part).await?;
        if self.tamper_files {
            for bytes in files.0.values_mut() {
                bytes.push(b'!');
            }
        }
        Ok(files)
    }

    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        if let Some(error) = &self.resolve_error {
            return Err(error.clone());
        }
        let mut parts = self.inner.ingredients(scope).await?;
        if let Some(tweak) = &self.tweak {
            parts.iter_mut().for_each(|p| tweak(p));
        }
        Ok(parts)
    }

    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        if let Some(error) = &self.withdrawn_error {
            return Err(error.clone());
        }
        self.inner.withdrawn(scope).await
    }

    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.repository_parts(repository).await
    }

    async fn vocabulary(
        &self,
        scope: &SourceScope,
    ) -> Result<ordin_agent_model::VocabularySet, PartSourceError> {
        self.inner.vocabulary(scope).await
    }

    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.used_by(part).await
    }

    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: ordin_agent_model::VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.citing_parts(repository, axis, code).await
    }
}

pub fn request(root_id: &str) -> ResolveRequest {
    ResolveRequest {
        scope: scope(),
        root_id: root_id.to_owned(),
        root_version: None,
        target_harness: HarnessKind::ClaudeCode,
        workspace_languages: Vec::new(),
        interface_language: "en-GB".to_owned(),
        pack_answers: Vec::new(),
        pack_changes: Vec::new(),
        overlay_changes: Vec::new(),
        replacements: Vec::new(),
        staged: Vec::new(),
        allow_draft_root: false,
        source: None,
    }
}

pub fn config(dir: &TempDir) -> ResolverConfig {
    ResolverConfig {
        bundle_root: dir.path().join("bundles"),
        compiler_version: "0.1.0-test".to_owned(),
    }
}

/// A resolver with `source` bound.
pub fn bound(dir: &TempDir, source: impl PartSource + 'static) -> ResolverAndCompiler {
    let resolver = ResolverAndCompiler::new(config(dir)).expect("a resolver");
    resolver
        .install_part_source(Arc::new(source))
        .expect("the first binding");
    resolver
}

/// The error of a result that must have failed.
pub fn error_of<T: std::fmt::Debug>(
    result: Result<T, ordin_resolver::ResolveError>,
) -> ordin_resolver::ResolveError {
    match result {
        Ok(value) => panic!("expected an error, got {value:?}"),
        Err(error) => error,
    }
}

/// The names of the entries of a directory, sorted.
pub fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

pub mod fixtures;
