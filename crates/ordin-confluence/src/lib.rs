//! The Confluence Adapter (ConfluenceAdapter.md): reads from and publishes to the company's Confluence
//! Cloud site with the person's own Atlassian token. It lists spaces and page trees, fetches pages and
//! attachments with their versions, finds what changed at a linked location, keeps a linked folder and
//! its published copies in step, makes and updates the page copies of Markdown originals for the Push
//! Service, and states who can see a space or page.
//!
//! The crate never writes to a place: fetched bytes go back to the Workspace Service. Its six tables
//! live in the Journal's SQLite datastore ([`ConfluenceAdapter::migrations`]), and every write is one
//! external-action bracket in the Journal, never resent automatically after a crash.
//!
//! Conditional assertions relied on (ConfluenceAdapter.md *Confirmation items bound here*): C-11 (a 403
//! on a write is `CONF_ACCESS_DENIED`), C-12 (the token arrives through Identity, whichever way the
//! sign-in works), C-13 (`docx` stays an attachment until `word_to_markdown` is wired) and C-42 (the
//! audience falls back to "everyone who can open it").
#![forbid(unsafe_code)]

mod audience;
mod client;
mod convert;
mod error;
mod linked;
mod push;
mod read;
mod store;
mod track;
mod types;
mod wire;

#[cfg(any(test, feature = "test-support"))]
pub use client::ConfluenceTestSeam;
pub use client::SCOPES;
pub use error::{ConfluenceError, ALL_CODES};
pub use types::{
    Audience, AutomaticPublishing, ConfluenceDeps, ConfluenceLocation, ConfluenceSettings,
    FetchedItem, LinkSyncReport, OriginChange, ReadyItem, ReadyKind, RemoteItem, RemoteItemRef,
    RemoteVersion, WordToMarkdown,
};

use std::sync::Arc;
#[cfg(any(test, feature = "test-support"))]
use std::time::Duration;

use ordin_approval::ApprovalService;
use ordin_files::FileService;
use ordin_journal::{EventSink, Journal, ModuleMigrations};
use sqlx::SqlitePool;

use crate::client::Transport;

const SQLITE_MIGRATIONS: &[ordin_journal::MigrationStep] = &[
    (1, "confluence tables", include_str!("sql/sqlite_v1.sql")),
    (
        2,
        "confluence ready items changed",
        include_str!("sql/sqlite_v2.sql"),
    ),
];

/// The Confluence Adapter. Constructed only by the desktop program; the ordin server has none.
pub struct ConfluenceAdapter {
    pub(crate) journal: Arc<Journal>,
    pub(crate) approvals: Arc<ApprovalService>,
    pub(crate) files: Arc<FileService>,
    pub(crate) transport: Transport,
    pub(crate) word_to_markdown: Option<Arc<dyn WordToMarkdown>>,
    /// `None` when the Journal's datastore is not SQLite: every table call is then
    /// `CONF_DATASTORE_UNAVAILABLE` (`new` has no error path, and the crate is desktop only).
    pool: Option<SqlitePool>,
}

impl ConfluenceAdapter {
    /// Module `confluence`; the program passes it to `MigrationRegistry::register`. Two SQLite steps
    /// and no PostgreSQL step: the adapter runs on no agent-profile server.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-04
    pub fn migrations() -> ModuleMigrations {
        ModuleMigrations {
            module: "confluence",
            sqlite: SQLITE_MIGRATIONS,
            postgres: &[],
        }
    }

    pub fn new(deps: ConfluenceDeps) -> Arc<ConfluenceAdapter> {
        let transport = Transport::new(deps.identity, deps.http, deps.settings);
        Arc::new(ConfluenceAdapter {
            pool: deps.journal.datastore().sqlite().cloned(),
            journal: deps.journal,
            approvals: deps.approvals,
            files: deps.files,
            transport,
            word_to_markdown: deps.word_to_markdown,
        })
    }

    // Points the adapter at a loopback stand-in, scales every wait and sets the timeouts, so a test need
    // not sit through 1 s, 2 s, 4 s, 30 s and 120 s.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_stand_in(
        deps: ConfluenceDeps,
        gateway: &str,
        wait_scale: f64,
        timeout: Duration,
    ) -> Arc<ConfluenceAdapter> {
        let mut transport = Transport::new(deps.identity, deps.http, deps.settings);
        transport.use_stand_in(gateway, wait_scale, timeout);
        Arc::new(ConfluenceAdapter {
            pool: deps.journal.datastore().sqlite().cloned(),
            journal: deps.journal,
            approvals: deps.approvals,
            files: deps.files,
            transport,
            word_to_markdown: deps.word_to_markdown,
        })
    }

    /// Puts a test's gateway, site and token in place of Atlassian's gateway, the profile's site and
    /// Identity's token for every later request (CONF-SEAM-2), on an adapter the program already
    /// built. Compiled only with the `test-support` feature, which no program enables.
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF-SEAM-2
    #[cfg(any(test, feature = "test-support"))]
    pub fn use_test_seam(&self, seam: ConfluenceTestSeam) -> Result<(), ConfluenceError> {
        self.transport.use_test_seam(seam)
    }

    /// The sink the program registers with `Journal::subscribe`; it acts on `approval_decided` for the
    /// approval ids stored in `confluence_publish_requests`.
    // spec: ordin-desktop/components/ConfluenceAdapter#SA-RECON-06
    pub fn event_sink(self: &Arc<Self>) -> Arc<dyn EventSink> {
        linked::sink(self)
    }

    pub(crate) fn pool(&self) -> Result<&SqlitePool, ConfluenceError> {
        self.pool
            .as_ref()
            .ok_or(ConfluenceError::DatastoreUnavailable)
    }
}
