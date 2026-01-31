//! The Catalogue Service (CatalogueService.md): the local, checked copy of every agent
//! repository the person reads, and the answers to every question asked of that library. It
//! implements the Agent Model's `PartSource`, through which the Resolver and Compiler and the
//! Checker fetch parts, and it owns the clones from which the Authoring Service's drafts branch.
//!
//! Tables live in the Journal's SQLite datastore as module `catalogue` ([`migrations`]); no
//! environment variable and no configuration key is read here.
#![forbid(unsafe_code)]

mod curation;
mod db;
mod drafts;
mod error;
mod git;
mod ingest;
mod library;
mod lifecycle;
mod partsource;
mod repos;
mod service;
mod types;
mod withdrawn;

pub use error::{CatalogueError, ALL_CODES};
pub use ordin_agent_model::BUNDLED_REPOSITORY_ID;
pub use service::{migrations, CatalogueDeps, CatalogueService};
pub use types::{
    AddRepositoryInput, AgentRepositoryRef, CatalogueConfig, CatalogueNotice, Coverage,
    DraftWorktree, DraftWorktreeInput, FetchOutcome, FetchResult, HandoverPlaceType, HealthLine,
    HealthReport, KindLine, LibraryEntry, LibraryFacets, LibraryQuery, LifecyclePlace, MadeOf,
    MatchRequirementsInput, PartDocument, PartHistory, PartStateLabel, PartVersionLine, PartView,
    ProbeRepositoryInput, PullRequestStatus, RenameRepositoryInput, RepositoryLayout,
    RepositoryProbe, RepositorySummary, RepositorySyncOutcome, RepositorySyncReport,
    RepositorySyncResult, RequirementMatch, ReviewerInfo, UsedBy,
};
