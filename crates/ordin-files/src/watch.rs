//! Watching places (FileService.md Internal Logic 1.3, 5.1, 5.2, 5a): one `notify` watcher per
//! place root, a per-path debounce, and the start-up rescan.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};
use ordin_types::time::Timestamp;
use ordin_types::PlaceId;
use sqlx::Row;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;

use crate::attribution::LeftOver;
use crate::error::FileError;
use crate::log;
use crate::moving::Carry;
use crate::paths::{is_temp_name, relative_to};
use crate::service::{lock, FileService};
use crate::types::{PlaceChange, PlaceChangeKind};
use crate::versions::blocking;
use crate::{MAX_FOLDER_DEPTH, WATCH_DEBOUNCE};

/// One path a watcher reported, relative to its place.
pub(crate) struct WatchEvent {
    place_id: PlaceId,
    rel: String,
}

/// The watchers, the event pump and the background tasks of one service.
#[derive(Default)]
pub(crate) struct WatchState {
    watchers: HashMap<PlaceId, notify::RecommendedWatcher>,
    events: Option<mpsc::UnboundedSender<WatchEvent>>,
    stop: Option<watch::Sender<bool>>,
    tasks: Vec<JoinHandle<()>>,
}

impl FileService {
    /// Starts the watcher task and the start-up rescan of every watched place. Returns at once.
    pub async fn start(self: &Arc<Self>) -> Result<(), FileError> {
        self.ensure_pump();
        let left = self.left_over_claims().await?;
        let ids = self.watched_places().await?;
        let mut to_scan = Vec::new();
        for id in ids {
            let place = self.place_row(id).await?;
            let present = blocking({
                let root = place.root.clone();
                move || root.is_dir()
            })
            .await?;
            if !present {
                log::place_missing(id);
                continue;
            }
            if self.start_watcher(id, &place.root).is_ok() {
                to_scan.push(id);
            }
        }
        self.spawn_rescan(to_scan, left);
        Ok(())
    }

    pub(crate) async fn watched_places(&self) -> Result<Vec<PlaceId>, FileError> {
        let rows = sqlx::query("SELECT id FROM places WHERE watched = 1 ORDER BY added_at, id")
            .fetch_all(&self.pool)
            .await?;
        let mut ids = Vec::new();
        for row in &rows {
            let id: String = row.try_get("id")?;
            ids.push(PlaceId::parse(&id).map_err(|_| FileError::DatastoreUnavailable)?);
        }
        Ok(ids)
    }

    /// Flushes pending index writes and stops the watcher (SA-XC-13).
    // spec: ordin-desktop/components/FileService#SA-XC-13
    pub async fn shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let (stop, tasks) = {
            let mut state = lock(&self.watching);
            state.watchers.clear();
            state.events = None;
            (state.stop.take(), std::mem::take(&mut state.tasks))
        };
        if let Some(stop) = stop {
            let _ = stop.send(true);
        }
        for task in tasks {
            let _ = task.await;
        }
    }

    pub async fn watch(&self, place_ids: &[PlaceId]) -> Result<(), FileError> {
        self.ensure_pump();
        let mut scan = Vec::new();
        for id in place_ids {
            let place = self.place_row(*id).await?;
            sqlx::query("UPDATE places SET watched = 1 WHERE id = ?1")
                .bind(id.to_string())
                .execute(&self.pool)
                .await?;
            self.start_watcher(*id, &place.root)?;
            if place.index_state == "none" {
                scan.push(*id);
            }
        }
        self.spawn_rescan(scan, LeftOver::default());
        Ok(())
    }

    /// Stops the watchers and sets `watched = 0`; the index is kept for search.
    pub async fn unwatch(&self, place_ids: &[PlaceId]) {
        for id in place_ids {
            lock(&self.watching).watchers.remove(id);
            let done = sqlx::query("UPDATE places SET watched = 0 WHERE id = ?1")
                .bind(id.to_string())
                .execute(&self.pool)
                .await;
            if let Err(error) = done {
                tracing::warn!(
                    component = crate::log::COMPONENT,
                    error_class = crate::error::db_class(&error),
                    "files.datastore_failed"
                );
            }
        }
    }

    pub fn subscribe_changes(&self) -> broadcast::Receiver<PlaceChange> {
        self.changes.subscribe()
    }

    /// Creates the event pump on first use.
    fn ensure_pump(&self) {
        let Some(service) = self.me.upgrade() else {
            return;
        };
        let mut state = lock(&self.watching);
        if state.events.is_some() {
            return;
        }
        self.stopping.store(false, Ordering::SeqCst);
        let (tx, rx) = mpsc::unbounded_channel();
        let (stop_tx, stop_rx) = watch::channel(false);
        state.events = Some(tx);
        state.stop = Some(stop_tx);
        state
            .tasks
            .push(tokio::spawn(async move { service.pump(rx, stop_rx).await }));
    }

    /// Rescans `ids`. Claims the last process left are settled first, and its wide claims close for
    /// the places a rescan covers (Internal Logic 6a); a failure keeps them for the next start.
    fn spawn_rescan(&self, ids: Vec<PlaceId>, left: LeftOver) {
        let Some(service) = self.me.upgrade() else {
            return;
        };
        let task = tokio::spawn(async move {
            let mut scanned = service.settle_left_over(&left).await.is_ok();
            for id in ids {
                if let Err(error) = service.rescan_place(id).await {
                    log::watch_failed(id, error_class(&error));
                    scanned = false;
                }
            }
            if scanned && service.close_left_over(left).await.is_err() {
                tracing::warn!(component = crate::log::COMPONENT, "files.claims_left_open");
            }
        });
        lock(&self.watching).tasks.push(task);
    }

    /// One `notify` watcher on the place root, recursive (Internal Logic 1.3).
    // spec: ordin-desktop/components/FileService#SA-RECON-09
    fn start_watcher(&self, place_id: PlaceId, root: &std::path::Path) -> Result<(), FileError> {
        let sender = lock(&self.watching)
            .events
            .clone()
            .ok_or(FileError::WatchFailed)?;
        let base = root.to_path_buf();
        let mut watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                let Ok(event) = result else { return };
                if matches!(event.kind, EventKind::Access(_)) {
                    return;
                }
                for path in &event.paths {
                    let Some(rel) = relative_to(&base, path) else {
                        continue;
                    };
                    if rel.is_empty() || rel.split('/').count() > MAX_FOLDER_DEPTH + 1 {
                        continue;
                    }
                    if rel.split('/').any(is_temp_name) {
                        continue;
                    }
                    let _ = sender.send(WatchEvent { place_id, rel });
                }
            })
            .map_err(|_| {
                log::watch_failed(place_id, "start");
                FileError::WatchFailed
            })?;
        watcher.watch(root, RecursiveMode::Recursive).map_err(|_| {
            log::watch_failed(place_id, "watch");
            FileError::WatchFailed
        })?;
        lock(&self.watching).watchers.insert(place_id, watcher);
        Ok(())
    }

    /// Debounces per path, then handles each place's ready paths together so a rename's two
    /// halves are seen in one batch.
    async fn pump(
        self: Arc<Self>,
        mut rx: mpsc::UnboundedReceiver<WatchEvent>,
        mut stop: watch::Receiver<bool>,
    ) {
        let mut pending: HashMap<(PlaceId, String), Instant> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    pending.insert((event.place_id, event.rel), Instant::now());
                }
                _ = tick.tick() => {
                    self.flush_ready(&mut pending, false).await;
                }
                _ = stop.changed() => {
                    while let Ok(event) = rx.try_recv() {
                        pending.insert((event.place_id, event.rel), Instant::now());
                    }
                    self.flush_ready(&mut pending, true).await;
                    break;
                }
            }
        }
    }

    async fn flush_ready(&self, pending: &mut HashMap<(PlaceId, String), Instant>, force: bool) {
        let ready: Vec<(PlaceId, String)> = pending
            .iter()
            .filter(|(_, at)| force || at.elapsed() >= WATCH_DEBOUNCE)
            .map(|(key, _)| key.clone())
            .collect();
        let mut by_place: HashMap<PlaceId, Vec<String>> = HashMap::new();
        for key in ready {
            pending.remove(&key);
            by_place.entry(key.0).or_default().push(key.1);
        }
        for (place_id, mut paths) in by_place {
            paths.sort();
            if let Err(error) = self.process_batch(place_id, paths).await {
                log::watch_failed(place_id, error_class(&error));
            }
        }
    }

    /// Internal Logic 5.2: stat each path; gone, new or changed. A vanished and an appeared file
    /// with the same bytes are a rename (5a).
    pub(crate) async fn process_batch(
        &self,
        place_id: PlaceId,
        paths: Vec<String>,
    ) -> Result<(), FileError> {
        let place = self.place_row(place_id).await?;
        let mut removed: Vec<(String, crate::index::IndexRow)> = Vec::new();
        let mut created: Vec<String> = Vec::new();
        let mut modified: Vec<String> = Vec::new();
        for rel in paths {
            if self.is_quiet(place_id, &rel) {
                continue;
            }
            let abs = place.root.join(&rel);
            let probe = abs.clone();
            let kind = blocking(move || {
                std::fs::symlink_metadata(&probe).ok().map(|m| {
                    (
                        m.is_dir(),
                        m.is_file(),
                        i64::try_from(m.len()).unwrap_or(i64::MAX),
                        crate::versions::mtime_ns(&m),
                    )
                })
            })
            .await?;
            match kind {
                None => {
                    if let Some(row) = self.index_row(place_id, &rel).await? {
                        removed.push((rel, row));
                    } else {
                        let folder = format!("{rel}/");
                        for file in self.remove_index_prefix(place_id, &folder).await? {
                            self.emit(removed_change(place_id, &file));
                        }
                    }
                }
                Some((true, _, _, mtime)) => {
                    let folder = format!("{rel}/");
                    if self.index_row(place_id, &folder).await?.is_none() {
                        self.index_folder(place_id, &rel, mtime).await?;
                        self.scan_new_folder(&place, &rel).await?;
                    }
                }
                Some((false, true, size, mtime)) => match self.index_row(place_id, &rel).await? {
                    Some(row) if row.size == size && row.mtime_ns == mtime => {}
                    Some(_) => modified.push(rel),
                    None => created.push(rel),
                },
                Some(_) => {}
            }
        }
        // Pair vanished and appeared files by their bytes.
        let mut renamed: Vec<(String, String)> = Vec::new();
        if !removed.is_empty() && !created.is_empty() {
            let mut hashes: HashMap<String, String> = HashMap::new();
            for rel in &created {
                let target = place.root.join(rel);
                if let Some(hash) = blocking(move || {
                    std::fs::read(&target)
                        .ok()
                        .map(|b| crate::versions::hash_of(&b))
                })
                .await?
                {
                    hashes.insert(rel.clone(), hash);
                }
            }
            for (from, row) in &removed {
                let Some(old_hash) = &row.hash else { continue };
                let matches: Vec<&String> = created
                    .iter()
                    .filter(|c| hashes.get(*c) == Some(old_hash))
                    .collect();
                if let [only] = matches.as_slice() {
                    renamed.push((from.clone(), (*only).clone()));
                }
            }
        }
        let renamed_from: Vec<&String> = renamed.iter().map(|(f, _)| f).collect();
        let renamed_to: Vec<&String> = renamed.iter().map(|(_, t)| t).collect();
        for (from, _) in &removed {
            if renamed_from.contains(&from) {
                continue;
            }
            self.remove_index_path(place_id, from).await?;
            self.emit(removed_change(place_id, from));
        }
        for (from, to) in &renamed {
            self.rename_seen(&place, from, to).await?;
        }
        for rel in &created {
            if renamed_to.contains(&rel) {
                continue;
            }
            let abs = place.root.join(rel);
            if let Some(change) = self.index_changed_file(&place, rel, &abs, true).await? {
                self.emit(change);
            }
        }
        for rel in &modified {
            let abs = place.root.join(rel);
            if let Some(change) = self.index_changed_file(&place, rel, &abs, false).await? {
                self.emit(change);
            }
        }
        Ok(())
    }

    /// A rename made by another app: the history follows the file before `Renamed` is announced.
    async fn rename_seen(
        &self,
        place: &crate::service::PlaceRow,
        from: &str,
        to: &str,
    ) -> Result<(), FileError> {
        match self.carry_history(place.id, from, to).await? {
            Carry::Moved => {}
            Carry::Collision => {
                // Both histories stay where they are.
                log::file_move_history_kept(place.id);
                self.remove_index_path(place.id, from).await?;
                let abs = place.root.join(to);
                let _ = self.index_changed_file(place, to, &abs, true).await?;
            }
        }
        let latest = self.latest(place.id, to).await?;
        self.emit(PlaceChange {
            place_id: place.id,
            path: to.to_owned(),
            change: PlaceChangeKind::Renamed {
                from: from.to_owned(),
            },
            observed_at: Timestamp::now(),
            by_ordin: false,
            version: latest.as_ref().map(|v| v.version),
            origin: latest.map(|v| v.origin),
        });
        Ok(())
    }

    /// Files and folders inside a folder that appeared whole (moved in, unpacked).
    async fn scan_new_folder(
        &self,
        place: &crate::service::PlaceRow,
        rel: &str,
    ) -> Result<(), FileError> {
        let root = place.root.join(rel);
        let stop = self.stopping.clone();
        let (walked, _) = blocking(move || crate::index::walk(&root, &stop)).await?;
        for entry in walked {
            let inner = format!("{rel}/{}", entry.rel);
            if entry.is_folder {
                self.index_folder(place.id, &inner, entry.mtime_ns).await?;
            } else {
                let abs = place.root.join(&inner);
                if let Some(change) = self.index_changed_file(place, &inner, &abs, true).await? {
                    self.emit(change);
                }
            }
        }
        Ok(())
    }
}

fn removed_change(place_id: PlaceId, path: &str) -> PlaceChange {
    PlaceChange {
        place_id,
        path: path.to_owned(),
        change: PlaceChangeKind::Removed,
        observed_at: Timestamp::now(),
        by_ordin: false,
        version: None,
        origin: None,
    }
}

fn error_class(error: &FileError) -> &'static str {
    match error {
        FileError::DatastoreUnavailable => "datastore",
        FileError::WatchFailed => "watch",
        FileError::NotFound => "not_found",
        _ => "other",
    }
}
