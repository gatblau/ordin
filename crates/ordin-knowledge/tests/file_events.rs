//! The Knowledge Service's file events against the real File Service (KnowledgeService.md *Internal
//! Logic* C-1 and C-3, and the ADR-104 amendment's C-1 step 3 and Q-1). The File Service runs over
//! the real Journal, Rules Engine and Agent Folder Manager on SQLite (`push_stack`), its watcher not
//! started, so every change event here is one the File Service emits for its own writes and moves.
//! The Workspace, Rules, Memory, Push, Folders and Identity ports are the fakes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/push_stack.rs"]
mod push_stack;
mod support;

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_files::{
    AddPlace, BinTicket, Cursor, FileBytes, FileEntry, FileError, FileService, FileVersion,
    FolderSort, Page, PlaceChange, PlaceChangeKind, PlaceKind, WriteVersion,
};
use ordin_knowledge::{
    AgentKnowledgeFile, KnowledgeFiles, KnowledgeService, ProgramKind, WritePage,
};
use ordin_types::domain::places::{
    FileVersionOrigin, KnowledgeAuthor, KnowledgePageState, KnowledgeSource,
};
use ordin_types::{AgentInstanceId, KnowledgePageId, PlaceId, RunId, WorkspaceId};
use sqlx::SqlitePool;
use support::fakes::{journal_entries, Fakes};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

const RATE_CARD: &str = "Suppliers/Rate card.csv";
const PAGE_FILE: &str = "knowledge/Supplier rates.md";

/// `KnowledgeFiles` as the desktop program wires it (`apps/desktop/src/hosts_knowledge.rs`): each
/// File Service call delegated. When `held` is set, `subscribe_changes` hands out that receiver
/// once instead of a new one, so a test can let it fall behind before the service reads it.
struct RealFiles {
    files: Arc<FileService>,
    held: Mutex<Option<broadcast::Receiver<PlaceChange>>>,
    subscribed: AtomicBool,
}

impl RealFiles {
    fn new(files: Arc<FileService>) -> Arc<RealFiles> {
        Arc::new(RealFiles {
            files,
            held: Mutex::new(None),
            subscribed: AtomicBool::new(false),
        })
    }
}

#[async_trait::async_trait]
impl KnowledgeFiles for RealFiles {
    async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError> {
        self.files.write_version(w).await
    }
    async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError> {
        self.files.read_file(place_id, path).await
    }
    async fn current_version(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<FileVersion>, FileError> {
        self.files.current_version(place_id, path).await
    }
    async fn move_to_bin(&self, place_id: PlaceId, path: &str) -> Result<BinTicket, FileError> {
        self.files.move_to_bin(place_id, path).await
    }
    async fn restore_from_bin(&self, ticket: &BinTicket) -> Result<FileVersion, FileError> {
        self.files.restore_from_bin(ticket).await
    }
    fn subscribe_changes(&self) -> broadcast::Receiver<PlaceChange> {
        let held = self.held.lock().unwrap().take();
        let rx = held.unwrap_or_else(|| self.files.subscribe_changes());
        self.subscribed.store(true, Ordering::SeqCst);
        rx
    }
    async fn list_agent_knowledge(
        &self,
        _agent: AgentInstanceId,
    ) -> Result<Vec<AgentKnowledgeFile>, FileError> {
        Ok(Vec::new())
    }
    async fn read_agent_knowledge(
        &self,
        _agent: AgentInstanceId,
        _relative_path: &str,
    ) -> Result<Vec<u8>, FileError> {
        Err(FileError::NotFound)
    }
    async fn move_file(
        &self,
        place_id: PlaceId,
        from: &str,
        to: &str,
    ) -> Result<FileVersion, FileError> {
        self.files.move_file(place_id, from, to).await
    }
    async fn versions(&self, place_id: PlaceId, path: &str) -> Result<Vec<FileVersion>, FileError> {
        self.files.versions(place_id, path).await
    }
    async fn list_folder(
        &self,
        place_id: PlaceId,
        folder: &str,
        sort: FolderSort,
        cursor: Option<Cursor>,
    ) -> Result<Page<FileEntry>, FileError> {
        self.files.list_folder(place_id, folder, sort, cursor).await
    }
}

struct Rig {
    stack: push_stack::Stack,
    real: Arc<RealFiles>,
    place: PlaceId,
    w: WorkspaceId,
    svc: Arc<KnowledgeService>,
    shutdown: CancellationToken,
}

impl Rig {
    /// The stack, one workspace whose own folder is a real `workspace_folder` place, and the
    /// Knowledge Service over the real File Service. `run` is not started yet.
    async fn new() -> Rig {
        let stack = push_stack::Stack::new(KnowledgeService::migrations()).await;
        let folder = stack.dir.0.join("Q3 supplier review");
        std::fs::create_dir_all(&folder).expect("the workspace folder");
        let place = stack
            .files
            .add_place(AddPlace {
                path: folder,
                friendly_name: Some("Q3 supplier review".to_owned()),
                kind: PlaceKind::WorkspaceFolder,
            })
            .await
            .expect("add the place")
            .id;
        let fakes = Fakes::new();
        let w = fakes.workspaces.add("Q3 supplier review", place);
        let real = RealFiles::new(stack.files.clone());
        let mut deps = fakes.deps(stack.journal.clone(), None);
        deps.files = real.clone();
        let svc = KnowledgeService::new(ProgramKind::Desktop, deps)
            .await
            .expect("the Knowledge Service");
        Rig {
            stack,
            real,
            place,
            w,
            svc,
            shutdown: CancellationToken::new(),
        }
    }

    /// Starts the listeners and returns once the file listener holds its receiver.
    async fn run(&self) {
        let svc = self.svc.clone();
        let token = self.shutdown.clone();
        tokio::spawn(async move { svc.run(token).await });
        let real = self.real.clone();
        until("the file listener subscribes", move || {
            let real = real.clone();
            async move { real.subscribed.load(Ordering::SeqCst) }
        })
        .await;
    }

    fn pool(&self) -> &SqlitePool {
        &self.stack.pool
    }

    /// A person's edit of a file of the place, as the files screen writes it.
    async fn person_writes(&self, path: &str, text: &str) -> FileVersion {
        self.stack
            .files
            .write_person_edit(self.place, path, text.as_bytes().to_vec())
            .await
            .expect("the person's write")
    }

    async fn rated_page(&self) -> KnowledgePageId {
        self.person_writes(RATE_CARD, "supplier,rate\nKestrel,410\n")
            .await;
        self.svc
            .write_page(
                KnowledgeAuthor::Ordin,
                WritePage {
                    workspace_id: self.w,
                    page_id: None,
                    title: "Supplier rates".to_owned(),
                    body_markdown: "## Review\nRates are reviewed each quarter.".to_owned(),
                    sources: vec![KnowledgeSource {
                        origin: "place".to_owned(),
                        location: format!("place:{}:{RATE_CARD}", self.place),
                        version_read: "1".to_owned(),
                    }],
                    run_id: Some(RunId::new()),
                    tags: None,
                    aliases: None,
                    page_type: None,
                    description: None,
                },
            )
            .await
            .expect("write the page")
    }

    /// (relative_path, title, state, written_by, edited_by_person, file_version) of a page row.
    async fn row(&self, id: KnowledgePageId) -> (String, String, String, String, i64, i64) {
        sqlx::query_as(
            "SELECT relative_path, title, state, written_by, edited_by_person, file_version \
             FROM knowledge_pages WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_one(self.pool())
        .await
        .expect("the page row")
    }

    async fn page_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_pages WHERE state <> 'removed'")
            .fetch_one(self.pool())
            .await
            .expect("the page count")
    }

    async fn journal_kinds(&self) -> Vec<String> {
        journal_entries(&self.stack.store)
            .await
            .into_iter()
            .map(|(kind, _)| kind)
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.stack.shutdown.cancel();
    }
}

/// Waits up to ten seconds for `check` to hold.
async fn until<F, Fut>(what: &str, check: F)
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let wait = async {
        while !check().await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    if tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .is_err()
    {
        panic!("timed out waiting for {what}");
    }
}

/// The next change `pick` accepts on `rx`, within ten seconds.
async fn next_change(
    rx: &mut broadcast::Receiver<PlaceChange>,
    pick: impl Fn(&PlaceChange) -> bool,
) -> PlaceChange {
    let wait = async {
        loop {
            match rx.recv().await {
                Ok(change) if pick(&change) => return change,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("change channel: {e}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("timed out waiting for a change")
}

/// C-1: the File Service announces the service's own write of a page file, and the service leaves
/// the page as it wrote it: the version is one it wrote, so the file is neither read back as a
/// person's edit nor adopted as a second page.
// spec: ordin-desktop/components/KnowledgeService#SA-RECON-08
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_services_own_write_is_announced_and_leaves_the_page_as_written() {
    let rig = Rig::new().await;
    rig.run().await;
    let mut seen = rig.stack.files.subscribe_changes();
    let id = rig.rated_page().await;

    let own = next_change(&mut seen, |c| c.path == PAGE_FILE).await;
    assert_eq!(own.change, PlaceChangeKind::Created);
    assert_eq!(own.version, Some(1));
    assert!(matches!(own.origin, Some(FileVersionOrigin::Run { .. })));

    // A person's new file after it: the listener takes changes in order, so once this one is
    // adopted the own write has been dealt with.
    rig.person_writes(
        "knowledge/Purchasing policy.md",
        "Orders over 5,000 need two quotes.\n",
    )
    .await;
    // The page row is committed before `knowledge_page_adopted` is journalled, so wait for both.
    let r = &rig;
    until("the person's file is adopted", move || async move {
        r.page_count().await == 2
            && r.journal_kinds()
                .await
                .iter()
                .any(|k| k == "knowledge_page_adopted")
    })
    .await;

    let (path, title, state, written_by, edited_by_person, file_version) = rig.row(id).await;
    assert_eq!(path, PAGE_FILE);
    assert_eq!(title, "Supplier rates");
    assert_eq!(state, "current");
    assert_eq!(written_by, "ordin");
    assert_eq!(edited_by_person, 0);
    assert_eq!(file_version, 1);
    let kinds = rig.journal_kinds().await;
    assert_eq!(
        kinds
            .iter()
            .filter(|k| *k == "knowledge_page_adopted")
            .count(),
        1,
        "{kinds:?}"
    );
    assert!(!kinds.contains(&"knowledge_header_invalid".to_owned()));
}

/// C-1 step 3 (ADR-104): `move_file` on the File Service announces `Renamed { from }`, and the page
/// follows its file: same page id, the new path, and the new file name as its title, written into
/// the header as a new version of the file at its new path.
// spec: ordin-desktop/components/KnowledgeService#ADR-104
// A current-thread runtime, so the log capture sees the start-up scan's events.
#[tokio::test]
async fn move_file_announces_renamed_and_the_page_follows_its_file() {
    let (logs, _guard) = support::logs::capture();
    let rig = Rig::new().await;
    let id = rig.rated_page().await;
    rig.run().await;
    // The start-up scan lists `knowledge/` in the background. Moving while it runs lets it find the
    // file at its new path before the history follows, adopt it there, and so fail the move with
    // `MoveTargetExists`; the move is made once the scan has finished.
    until("the start-up scan finishes", || {
        let logs = logs.clone();
        async move { !logs.named("knowledge_scan_finished").is_empty() }
    })
    .await;
    let mut seen = rig.stack.files.subscribe_changes();

    rig.stack
        .files
        .move_file(rig.place, PAGE_FILE, "knowledge/Day rates.md")
        .await
        .expect("move the page file");
    let moved = next_change(&mut seen, |c| {
        matches!(c.change, PlaceChangeKind::Renamed { .. })
    })
    .await;
    assert_eq!(moved.path, "knowledge/Day rates.md");
    assert_eq!(
        moved.change,
        PlaceChangeKind::Renamed {
            from: PAGE_FILE.to_owned()
        }
    );

    let r = &rig;
    until("the page follows its file", move || async move {
        r.row(id).await.1 == "Day rates"
    })
    .await;
    let (path, _, state, _, edited_by_person, file_version) = rig.row(id).await;
    assert_eq!(path, "knowledge/Day rates.md");
    assert_eq!(state, "current");
    assert_eq!(edited_by_person, 1);
    assert_eq!(rig.page_count().await, 1);
    let file = rig
        .stack
        .files
        .read_file(rig.place, "knowledge/Day rates.md")
        .await
        .expect("the moved file");
    let text = String::from_utf8(file.bytes).expect("text");
    assert!(text.contains("title: Day rates"), "{text}");
    assert!(text.contains(&format!("page_id: {id}")), "{text}");
    let versions = rig
        .stack
        .files
        .versions(rig.place, "knowledge/Day rates.md")
        .await
        .expect("the versions");
    assert_eq!(versions.len(), 2, "the written version and the title line");
    assert_eq!(
        i64::from(versions.iter().map(|v| v.version).max().unwrap_or(0)),
        file_version
    );
    assert!(!rig
        .journal_kinds()
        .await
        .contains(&"knowledge_page_removed_outside".to_owned()));
}

/// Q-1 (ADR-104): a rename in ordin moves the file with `move_file`; the `Renamed` it announces
/// finds the page already at the new path, so nothing more happens to it.
// spec: ordin-desktop/components/KnowledgeService#ADR-104
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rename_in_ordin_moves_the_file_and_its_own_renamed_changes_nothing() {
    let rig = Rig::new().await;
    let id = rig.rated_page().await;
    rig.run().await;
    let mut seen = rig.stack.files.subscribe_changes();

    rig.svc
        .save_edit(
            id,
            "Supplier day rates".to_owned(),
            "## Review\nRates are reviewed each quarter.".to_owned(),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("save with a new title");
    let moved = next_change(&mut seen, |c| {
        matches!(c.change, PlaceChangeKind::Renamed { .. })
    })
    .await;
    assert_eq!(moved.path, "knowledge/Supplier day rates.md");
    assert_eq!(
        moved.change,
        PlaceChangeKind::Renamed {
            from: PAGE_FILE.to_owned()
        }
    );

    // A person's new file after it, as in the own-write test: once adopted, the Renamed is done.
    rig.person_writes(
        "knowledge/Purchasing policy.md",
        "Orders over 5,000 need two quotes.\n",
    )
    .await;
    // The page row is committed before `knowledge_page_adopted` is journalled, so wait for both.
    let r = &rig;
    until("the person's file is adopted", move || async move {
        r.page_count().await == 2
            && r.journal_kinds()
                .await
                .iter()
                .any(|k| k == "knowledge_page_adopted")
    })
    .await;
    let (path, title, state, _, _, _) = rig.row(id).await;
    assert_eq!(path, "knowledge/Supplier day rates.md");
    assert_eq!(title, "Supplier day rates");
    assert_eq!(state, "current");
    let versions = rig
        .stack
        .files
        .versions(rig.place, "knowledge/Supplier day rates.md")
        .await
        .expect("the versions");
    assert!(versions.len() >= 2, "the history moved with the file");
    let kinds = rig.journal_kinds().await;
    assert!(
        kinds.contains(&"knowledge_page_renamed".to_owned()),
        "{kinds:?}"
    );
    assert!(!kinds.contains(&"knowledge_page_removed_outside".to_owned()));
}

/// C-3: the service's receiver of the File Service's changes falls behind (`Lagged`), and the full
/// re-check marks the page whose cited file moved on. The changes are the File Service's own,
/// relayed into a one-place channel that nobody reads until the service starts; the one change left
/// in it is about another file, so only the re-check can mark the page.
// spec: ordin-desktop/components/KnowledgeService#SA-RECON-08
#[tokio::test]
async fn a_lagged_change_stream_triggers_the_resync_that_marks_the_page() {
    let (logs, _guard) = support::logs::capture();
    let rig = Rig::new().await;
    let (relay, held) = broadcast::channel::<PlaceChange>(1);
    *rig.real.held.lock().unwrap() = Some(held);
    let forwarded = Arc::new(AtomicUsize::new(0));
    {
        let mut source = rig.stack.files.subscribe_changes();
        let forwarded = forwarded.clone();
        tokio::spawn(async move {
            while let Ok(change) = source.recv().await {
                let _ = relay.send(change);
                forwarded.fetch_add(1, Ordering::SeqCst);
            }
        });
    }

    let id = rig.rated_page().await;
    rig.person_writes(RATE_CARD, "supplier,rate\nKestrel,425\n")
        .await;
    rig.person_writes(RATE_CARD, "supplier,rate\nKestrel,440\n")
        .await;
    rig.person_writes("Notes/unrelated.txt", "Nothing to do with rates.\n")
        .await;
    // The rate card's first version, the page file, two rate card edits and the unrelated note.
    let relayed = forwarded.clone();
    until("the five changes are relayed", move || {
        let relayed = relayed.clone();
        async move { relayed.load(Ordering::SeqCst) == 5 }
    })
    .await;
    assert_eq!(rig.row(id).await.2, "current");

    rig.run().await;
    let r = &rig;
    until("the re-check marks the page", move || async move {
        r.row(id).await.2 == "may_be_out_of_date"
    })
    .await;
    let resyncs = logs.named("knowledge_freshness_resync");
    assert_eq!(resyncs.len(), 1, "{:?}", logs.all());
    assert_eq!(
        resyncs[0].fields.get("pages").map(String::as_str),
        Some("1")
    );
    let view = rig.svc.get_page(id).await.expect("the page");
    assert_eq!(view.page.state, KnowledgePageState::MayBeOutOfDate);
    assert_eq!(view.changes.len(), 1);
    assert_eq!(view.changes[0].version_read, "1");
    assert_eq!(view.changes[0].newer_version, "3");
}
