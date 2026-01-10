//! The Agent Model: the ordin format in Rust (AgentModel.md).
//!
//! The part-manifest model (`apiVersion: ai.enterprise/v1`, 25 kinds, overlays, vocabularies) plus the
//! one WorkspaceProfile field of ordin's own, `fixedPracticePacks`. It reads and writes part
//! manifests, groups the files of a repository tree into parts, renders the file of a part from its
//! fields, places a part's file where the format places it, and computes the canonical archive whose
//! SHA-256 is a part's content digest. It also defines the plan, bundle-manifest, lock-file and
//! resolved-tool-policy types, the withdrawn-list file, the draft model, and the `PartSource` trait
//! through which the Resolver and Compiler and the Checker read parts.
//!
//! A pure library: no I/O, no datastore, no model call, no environment variable, no log line
//! (Configuration: none; Logging: callers log the error code). YAML is parsed only through
//! `ordin_types::yaml::read_restricted_node`, which refuses anchors, aliases and custom tags, and is
//! emitted by this crate's own fixed code.
#![forbid(unsafe_code)]

mod archive;
mod draft;
mod error;
mod ident;
mod kinds;
mod manifest;
mod plan;
mod readers;
mod render;
mod source;
mod tree;
mod value;
mod vocabulary;
mod withdrawn;

#[cfg(feature = "testkit")]
pub mod testkit;

pub use ordin_types::domain::agents::{
    Part, PartKind, PartStatus, VocabularyAxis, WithdrawnListEntry,
};
pub use ordin_types::num::{DecimalString, Sha256Hex};
pub use ordin_types::time::Timestamp;
pub use ordin_types::{AgentRepositoryId, PartId};

pub use archive::{canonical_archive, content_digest, parse_prefixed_digest, prefixed_digest};
pub use draft::{DraftModel, FieldModel, GeneratedPart, ReusedPart};
pub use error::{AmError, PartSourceError, ALL_CODES};
pub use ident::{is_part_identifier, mint_identifier, slug_from_id, strip_kind_dir_prefix, Urn};
pub use kinds::{
    all_kinds, canonical_kind, kind_info, overlay_order, FileConvention, KindFamily, KindInfo,
    LoadingMode,
};
pub use manifest::{
    markdown_body, parse_manifest, relation_kind, Audience, Manifest, ManifestRef, Metadata,
    UseCase,
};
pub use plan::{
    canonical_json, compare_versions, BundleArtefact, BundleManifest, ChosenBy, LockFile,
    McpServerDecl, PartRef, PendingPackChoice, ResolutionPlan, ResolvedToolPolicy, SelectedPart,
    Selections,
};
pub use readers::{
    AppliesTo, ContractSection, ControlLoopSpec, FixedPackEntry, JourneyBody, JourneySlackChannel,
    JourneyStage, MemoryContract, OutputContract, OverlayOffer, PackToolchain, RequiredLoopRule,
    RequiredPackRule, SelectablePack, SendsBackEntry, StageNextEntry, ToolBindingSpec,
    VocabularyBody, VocabularyTermBody,
};
pub use render::{part_file_path, render_manifest_yaml, render_part, RenderedFile};
pub use source::{
    PartSource, RepositoryRef, Resolution, SourceOverride, SourceScope, SourcedPart, StagedPart,
};
pub use tree::{read_tree, ModelPart, PartFiles, TreeReading};
pub use value::YamlValue;
pub use vocabulary::VocabularySet;
pub use withdrawn::{
    parse_withdrawn_list, render_withdrawn_list, WithdrawnFileEntry, WithdrawnFilePart,
    WithdrawnFileReplacement, WithdrawnListFile,
};

pub const API_VERSION: &str = "ai.enterprise/v1";
/// Names of the agent folder's lock file and the bundle manifest.
pub const LOCK_FILE_NAME: &str = "ordin.lock";
pub const LOCK_FILE_SCHEMA: &str = "ordin.lock/v1";
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.manifest.json";
pub const BUNDLE_SCHEMA: &str = "ordin.bundle/v1";
/// `agent_repository_id` of every row of the bundled catalogue: a fixed UUIDv7-shaped value so the
/// Catalogue Service, the Resolver and the Checker agree on it without a lookup.
pub const BUNDLED_REPOSITORY_ID: AgentRepositoryId =
    AgentRepositoryId(uuid::uuid!("01900000-0000-7000-8000-000000000001"));
/// `listSchema` of `withdrawn.yaml` (08 *Withdrawn list*).
pub const WITHDRAWN_LIST_SCHEMA: &str = "ordin.withdrawn/v1";
pub const WITHDRAWN_LIST_FILE_NAME: &str = "withdrawn.yaml";
