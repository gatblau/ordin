//! The Push Service (PushService.md): the one way a change made in ordin leaves the computer for a
//! shared place. It groups the changed documents of one request into one approval card, checks that
//! the origin has not moved, and after approval sends each file to SharePoint, Confluence or a Git
//! remote as an external action with a unique key, so that a restart never sends twice. It also
//! opens pull requests, keeps published copies of Markdown originals up to date, and sends team
//! knowledge shares.
//!
//! Tables live in the Journal's datastore, registered as module `push` through
//! [`PushService::migrations`]. Approval decisions arrive through the service's
//! [`ordin_journal::EventSink`].
#![forbid(unsafe_code)]

mod copies;
mod documents;
mod error;
mod gitops;
mod model;
mod pulls;
mod service;
mod sink;
mod store;
mod text;

pub use error::{PushError, ALL_CODES};
pub use model::{
    ApprovedPullRequest, CardText, ChooseVersionInput, CopyAction, CopyActionInput, HeldChoice,
    ProposedItem, PublishCopyRequest, PullRequestContent, PullRequestProposal, PushDeps,
    PushNotice, PushProposal, StoreFailure, SubmissionId, SubmissionOrigin, TeamShareProposal,
    TryAgainInput, WorkingCopyEntry, WorkingCopyStore,
};
pub use service::PushService;
