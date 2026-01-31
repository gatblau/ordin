//! The Format Plug-ins (FormatPlugins.md): the files of an agent format turned into Agent Model
//! parts and back.
//!
//! One module per format, compiled into the build, each with four operations: *detect*, *read* (with
//! a mapping report of what has no equivalent), *write* (with a loss report of what the format cannot
//! hold) and *versions*. The first release has the ordin format (lossless), the APM format (read and
//! write, pinned to tested versions) and two read-only imports, Claude Code and Copilot. Every plug-in
//! is a pure function over an in-memory tree; the caller reads and writes disk and Git. Mapping and
//! loss reports are returned values: this crate has no Journal dependency.
//!
//! No run-time plug-in loading (ADR-087), no environment variable, no profile key (Configuration:
//! none). YAML is read only through `ordin_types::yaml::read_restricted_node`. Anything that does not
//! map is reported, never guessed; text read is data, never obeyed (R-84).
#![forbid(unsafe_code)]

mod apm;
mod claude;
mod copilot;
mod error;
mod front;
mod guard;
mod import;
mod log;
mod ordin;
mod plugins;
mod propose;
mod tree_source;
mod types;

pub use ordin_agent_model::{
    compare_versions, prefixed_digest, AgentRepositoryId, LoadingMode, Manifest, ModelPart, Part,
    PartFiles, PartId, PartKind, PartRef, PartSource, PartSourceError, PartStatus, RepositoryRef,
    Resolution, Sha256Hex, SourceScope, SourcedPart, TreeReading, VocabularyAxis, VocabularySet,
    WithdrawnListEntry, YamlValue, API_VERSION,
};
pub use ordin_types::domain::agents::AgentFormat;

pub use error::{FormatError, ALL_CODES};
pub use guard::{MAX_FILE_BYTES, MAX_TREE_BYTES, MAX_TREE_FILES};
pub use plugins::FormatPlugins;
pub use tree_source::TreeSource;
pub use types::{
    Detection, FormatId, FormatPlugin, FormatVersions, KindProposal, LossItem, LossReason,
    LossReport, MappingItem, MappingReport, PackageIdentity, ReadOutcome, ReadRequest, SourceTree,
    UnmappedReason, WriteOutcome, WriteRequest,
};
