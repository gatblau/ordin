//! The `PartSource` trait and the types around it (AgentModel.md *The `PartSource` trait*, *Staged
//! parts and per-request sources*, *Internal Logic* 12).
//!
//! The trait is the only shape of a part source. It is object-safe so the Checker and the Resolver
//! and Compiler can hold it in a slot bound once after the Catalogue Service is built (SA-RECON-01);
//! this crate defines no binding mechanism.

use std::sync::Arc;

use ordin_types::domain::agents::{Part, PartKind, VocabularyAxis, WithdrawnListEntry};
use ordin_types::AgentRepositoryId;

use crate::error::PartSourceError;
use crate::manifest::Manifest;
use crate::tree::{ModelPart, PartFiles};
use crate::vocabulary::VocabularySet;

/// Where one agent's references resolve, in order (ADR-090 §2): `home`, then `profile`
/// in the order the organisation profile lists them, then the bundled catalogue.
/// A repository the person added appears only as `home` of its own agents, never in `profile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScope {
    pub home: RepositoryRef,
    pub profile: Vec<RepositoryRef>,
    pub include_bundled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RepositoryRef {
    Repository(AgentRepositoryId),
    Bundled,
}

/// One part as the source holds it at its followed commit.
#[derive(Debug, Clone)]
pub struct SourcedPart {
    /// The catalogue row (2C `Part`); for the bundled catalogue, `agent_repository_id` is
    /// `BUNDLED_REPOSITORY_ID`.
    pub part: Part,
    pub repository: RepositoryRef,
    /// `<host>/<owner>/<name>` or `bundled` — the value written into `PartRef.repository`.
    pub repository_label: String,
    /// Display name shown as "From: [display name]"; "ordin" for the bundled catalogue.
    pub repository_display_name: String,
    pub manifest: Manifest,
}

// The variants are the spec's (`Found(SourcedPart)` unboxed); boxing would change the public
// interface every `PartSource` implementation and caller matches on.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Resolution {
    Found(SourcedPart),
    NotFound,
    /// Two repositories at the same step of the order carry the identifier (ADR-090 §2):
    /// refused for that part; the source reports it to Diagnostics.
    Ambiguous {
        repositories: Vec<String>,
    },
}

#[async_trait::async_trait]
pub trait PartSource: Send + Sync {
    /// The part with `kind` and `metadata.id` = `id`, at `version` when given, otherwise the
    /// highest semantic version whose status is not draft on the followed commit, resolved
    /// through `scope` in order. Withdrawn parts are returned with `part.withdrawn = true`.
    /// Only parts the source offers are visible: a part its ingest refused is `NotFound`, never an
    /// error.
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError>;

    /// The same lookup when the reference carries no kind: every kind is tried; two kinds matching
    /// is Ambiguous.
    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError>;

    /// The files of one part, exactly as committed. The caller recomputes `content_digest`
    /// and refuses a mismatch (AM_SOURCE_CORRUPT).
    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError>;

    /// Every PracticePack and every overlay visible through `scope`, latest non-draft version of
    /// each identifier, for pack rules by class and overlay offers.
    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError>;

    /// Every withdrawn-list entry in force in the repositories of `scope` (entries removed by
    /// hand included, R-78).
    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError>;

    /// Every part of one repository at its followed commit, for whole-repository checks.
    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError>;

    /// The vocabulary set of `scope` (home repository's vocabulary parts, then borrowed ones).
    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError>;

    /// Agents (WorkspaceProfiles) whose forward closure includes the part (reverse closure,
    /// 02:441 *Used by*), across every repository the source reads. Vocabulary citations are
    /// not references and are not followed here; `citing_parts` answers them.
    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError>;

    /// Every head part, in every repository the source reads, whose `vocabulary_terms_cited`
    /// includes the term `code` on `axis` of `repository`'s vocabulary (ADR-091 item 4).
    /// Sorted by (repository_label, kind, identifier). Called by: Checker (retiring a term).
    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError>;
}

/// A part that is not (yet) in the source: a draft's new or changed part. It shadows a source
/// part with the same kind and identifier. Built by the Authoring Service; read by the Checker
/// and the Resolver and Compiler (which re-exports it).
// spec: ordin-desktop/components/AgentModel#SA-CLOSE-04
#[derive(Debug, Clone)]
pub struct StagedPart {
    pub part: ModelPart,
    pub repository: RepositoryRef,
    /// `<host>/<owner>/<name>`.
    pub repository_label: String,
}

/// A source used for one request in place of the bound one: the Catalogue Service's
/// `draft_source(repository, worktree)` for a trial or a draft check (R18).
#[derive(Clone)]
pub struct SourceOverride(pub Arc<dyn PartSource>);

impl std::fmt::Debug for SourceOverride {
    /// Prints `SourceOverride(..)`; the source has no printable state.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SourceOverride(..)")
    }
}
