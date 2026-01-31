//! The Package Catalogue (PackageCatalogue.md): makes adding an agent one action, **Add it**, and
//! keeps the list of agent instances on this computer with their install records. For each add it
//! asks the Checker whether the agent's parts pass and fall inside the company's trust policy, has
//! the Resolver and Compiler build the bundle with the packs and overlays picked for the workspace,
//! has the Agent Folder Manager create the folder and the session harness's Installer write it, and
//! has the Package Inspector describe what the agent can do. It applies updates between runs when
//! what the agent may do or read is unchanged and asks the person otherwise, stops agents whose
//! parts are withdrawn, records the person's answer to a pack question, installs the person's own
//! draft as a trial and removes an agent after the person confirms. It never edits a part's files.
//!
//! Tables live in the Journal's SQLite datastore as module `package_catalogue` ([`migrations`]); no
//! environment variable and no configuration key is read here. The providers are held as private
//! port traits over their concrete types (PackageCatalogue.md *Boundary interfaces*).
#![forbid(unsafe_code)]

mod add;
mod bundled;
mod catalogue;
mod error;
mod manage;
mod ports;
mod request;
mod starter;
mod store;
mod types;
mod update;
mod views;

#[cfg(test)]
mod tests;

pub use catalogue::{
    migrations, HarnessSettings, InstanceActivity, PackageCatalogue, PackageCatalogueDeps,
};
pub use error::{PackageError, ALL_CODES};
pub use starter::STARTER_AGENTS;
pub use types::{
    AddAgentInput, AddOutcome, AddPreview, AgentFolderFailure, AgentFolderReport, AgentFound,
    AgentMatch, AgentNotice, AgentRepositoryRef, Availability, BundledAgent, ChangeChoiceInput,
    HarnessKind, InstalledAgent, PackChoice, PendingChoice, PendingChoiceOption, PendingUpdate,
    RemoveAgentInput, TrialInput, UsesItem, WorkspaceAgent,
};
