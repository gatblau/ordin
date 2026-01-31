//! The Agent Folder Manager: start, recovery, create, get, lock and remove
//! (AgentFolderManager.md Internal Logic 1–5). The grant methods are in `grants.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ordin_journal::{Datastore, Journal, NewJournalEntry};
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::{AgentInstanceId, CodedError, CrewCharterId, RunId, WorkspaceId};

use crate::db::{self, failed, Arg, DbRow, Tx};
use crate::error::AfmError;
use crate::lock::{acquire, FolderLease, InstanceLock, WRITE_LOCK_WAIT};
use crate::paths::{make_private_dir_all, remove_tree};
use crate::types::{AgentFolder, AgentFolderState, LockMode, RecoveryReport};
use crate::MODULE;

/// The directory under the application data root that holds one directory per agent instance
/// (*chosen here*).
pub const AGENTS_DIR: &str = "agents";

const SELECT_ROW: &str = "SELECT {s:folder_path}, {s:meta_path}, state, {t:created_at} \
     FROM agent_folders WHERE agent_instance_id = $1{uuid}";

/// A session opened by `apply_grants` and not yet ended (step 6a: "the in-memory list of sessions").
#[derive(Debug, Clone, Copy)]
pub(crate) struct LiveSession {
    pub(crate) agent_instance_id: AgentInstanceId,
    pub(crate) workspace_id: WorkspaceId,
    pub(crate) crew_charter_id: Option<CrewCharterId>,
}

/// One `agent_folders` row.
pub(crate) struct FolderRow {
    pub(crate) folder_path: PathBuf,
    pub(crate) meta_path: PathBuf,
    pub(crate) state: AgentFolderState,
    pub(crate) created_at: Timestamp,
}

impl FolderRow {
    fn parse(row: &DbRow) -> Result<FolderRow, AfmError> {
        let corrupt = || AfmError::DatastoreFailed { retryable: false };
        let state = match row.text(2).map_err(failed("read_agent_folder"))?.as_str() {
            "created" => AgentFolderState::Created,
            "removing" => AgentFolderState::Removing,
            "removed" => AgentFolderState::Removed,
            _ => return Err(corrupt()),
        };
        let created = row.text(3).map_err(failed("read_agent_folder"))?;
        let created_at = parse_rfc3339_ms_utc(&created)
            .map(Timestamp)
            .map_err(|_| corrupt())?;
        Ok(FolderRow {
            folder_path: PathBuf::from(row.text(0).map_err(failed("read_agent_folder"))?),
            meta_path: PathBuf::from(row.text(1).map_err(failed("read_agent_folder"))?),
            state,
            created_at,
        })
    }
}

/// Creates, locks and removes agent folders and applies the session-level folder grants.
/// Consumers hold `Arc<AgentFolderManager>` and call the inherent methods (SA-RECON-02).
// spec: ordin-desktop/components/AgentFolderManager#SA-RECON-02
pub struct AgentFolderManager {
    pub(crate) journal: Journal,
    pub(crate) store: Datastore,
    /// The canonical data root, resolved once at start (step 1).
    pub(crate) root: PathBuf,
    /// The data root as the program gave it; a granted path may be spelled through either form.
    pub(crate) given_root: PathBuf,
    agents_dir: PathBuf,
    locks: Mutex<HashMap<AgentInstanceId, Arc<InstanceLock>>>,
    pub(crate) live: Mutex<HashMap<RunId, LiveSession>>,
    /// One writer at a time inside this process; keeps the in-memory session list and the tables
    /// in step. The datastore's own transaction guards other processes.
    pub(crate) write_gate: tokio::sync::Mutex<()>,
    lock_wait: Duration,
}

pub(crate) fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl AgentFolderManager {
    /// Step 1. Creates `<data root>/agents` when missing (mode `0700` on macOS and Linux), confirms
    /// it is writable and keeps the canonical data root. Failure is `AFM_ROOT_UNWRITABLE`.
    pub async fn new(journal: Journal, data_root: PathBuf) -> Result<Self, AfmError> {
        let given_root = data_root.clone();
        let root = tokio::task::spawn_blocking(move || -> std::io::Result<PathBuf> {
            let agents = data_root.join(AGENTS_DIR);
            make_private_dir_all(&agents)?;
            let probe = agents.join(format!(".probe-{}", uuid::Uuid::now_v7()));
            std::fs::write(&probe, b"")?;
            std::fs::remove_file(&probe)?;
            // spec: ordin-desktop/components/AgentFolderManager#AFM-PATH-1
            std::fs::canonicalize(&data_root).map(ordin_types::paths::plain_form)
        })
        .await
        .map_err(|_| AfmError::RootUnwritable)?
        .map_err(|_| AfmError::RootUnwritable)?;
        if root.to_str().is_none() {
            return Err(AfmError::RootUnwritable);
        }
        let store = journal.datastore().clone();
        Ok(AgentFolderManager {
            journal,
            store,
            agents_dir: root.join(AGENTS_DIR),
            root,
            given_root,
            locks: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
            write_gate: tokio::sync::Mutex::new(()),
            lock_wait: WRITE_LOCK_WAIT,
        })
    }

    /// A shorter wait for the tests that provoke `AFM_FOLDER_BUSY`.
    #[cfg(test)]
    pub(crate) fn with_lock_wait(mut self, wait: Duration) -> Self {
        self.lock_wait = wait;
        self
    }

    /// The data root and every path inside it are never grantable (steps 6, 6a).
    pub(crate) fn ordin_roots(&self) -> [&Path; 2] {
        [&self.root, &self.given_root]
    }

    /// Step 2. Finishes interrupted removals and clears session applications left by a previous start.
    // spec: ordin-desktop/components/AgentFolderManager#SA-RECON-04
    pub async fn recover(&self) -> Result<RecoveryReport, AfmError> {
        let _gate = self.write_gate.lock().await;
        let mut report = RecoveryReport::default();
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_recover"))?;
        // Interrupted removals: repeat step 5's deletion and set `removed`.
        let removing = tx
            .fetch_all(
                "SELECT {s:agent_instance_id}, {s:folder_path}, {s:meta_path} FROM agent_folders \
                 WHERE state = 'removing'",
                &[],
            )
            .await
            .map_err(failed("select_removing"))?;
        for row in &removing {
            let id = row.text(0).map_err(failed("read_removing"))?;
            let folder = PathBuf::from(row.text(1).map_err(failed("read_removing"))?);
            let meta = PathBuf::from(row.text(2).map_err(failed("read_removing"))?);
            delete_instance_dirs(folder, meta).await?;
            tx.execute(
                "DELETE FROM session_folder_access WHERE agent_instance_id = $1{uuid}",
                &[Arg::text(&id)],
            )
            .await
            .map_err(failed("delete_session_access"))?;
            tx.execute(
                "UPDATE agent_folders SET state = 'removed', removed_at = $2{ts} \
                 WHERE agent_instance_id = $1{uuid}",
                &[Arg::text(&id), Arg::text(Timestamp::now().to_wire())],
            )
            .await
            .map_err(failed("finish_removal"))?;
            report.removals_finished += 1;
        }
        // Sessions do not survive a restart without the Session Supervisor applying grants again.
        let runs = tx
            .fetch_optional(
                "SELECT CAST(count(DISTINCT run_id) AS TEXT) FROM session_folder_access",
                &[],
            )
            .await
            .map_err(failed("count_sessions"))?;
        if let Some(row) = runs {
            let count = row.text(0).map_err(failed("count_sessions"))?;
            report.sessions_cleared = count.parse().unwrap_or(0);
        }
        tx.execute("DELETE FROM session_folder_access", &[])
            .await
            .map_err(failed("delete_session_access"))?;
        let known: HashSet<String> = tx
            .fetch_all("SELECT {s:agent_instance_id} FROM agent_folders", &[])
            .await
            .map_err(failed("select_known"))?
            .iter()
            .map(|row| row.text(0))
            .collect::<Result<_, _>>()
            .map_err(failed("read_known"))?;
        tx.commit().await.map_err(failed("commit_recover"))?;
        guard(&self.live).clear();
        // Directories ordin did not record are counted and left untouched.
        let agents = self.agents_dir.clone();
        let unknown = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<String>> {
            let mut names = Vec::new();
            for entry in std::fs::read_dir(agents)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if !known.contains(&name) {
                    names.push(name);
                }
            }
            Ok(names)
        })
        .await
        .map_err(|_| AfmError::io())?
        .map_err(|_| AfmError::io())?;
        for name in &unknown {
            crate::log::unknown_directory(name);
        }
        report.unknown_directories = u32::try_from(unknown.len()).unwrap_or(u32::MAX);
        Ok(report)
    }

    /// Step 3.
    pub async fn create(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError> {
        let started = std::time::Instant::now();
        let _gate = self.write_gate.lock().await;
        let instance_dir = self.agents_dir.join(id.to_string());
        let folder_path = instance_dir.join("folder");
        let meta_path = instance_dir.join("meta");
        let created_at = Timestamp::now();
        let (Some(folder_text), Some(meta_text)) = (folder_path.to_str(), meta_path.to_str())
        else {
            return Err(AfmError::RootUnwritable);
        };
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_create"))?;
        let existing = tx
            .fetch_optional(
                "SELECT state FROM agent_folders WHERE agent_instance_id = $1{uuid}",
                &[Arg::text(id)],
            )
            .await
            .map_err(failed("select_state"))?;
        if let Some(row) = existing {
            match row.text(0).map_err(failed("select_state"))?.as_str() {
                "removed" => {
                    tx.execute(
                        "DELETE FROM agent_folders WHERE agent_instance_id = $1{uuid}",
                        &[Arg::text(id)],
                    )
                    .await
                    .map_err(failed("delete_removed"))?;
                }
                _ => return Err(AfmError::FolderExists { id }),
            }
        }
        tx.execute(
            "INSERT INTO agent_folders (agent_instance_id, folder_path, meta_path, state, created_at) \
             VALUES ($1{uuid}, $2, $3, 'created', $4{ts})",
            &[
                Arg::text(id),
                Arg::text(folder_text),
                Arg::text(meta_text),
                Arg::text(created_at.to_wire()),
            ],
        )
        .await
        .map_err(failed("insert_agent_folder"))?;
        // An I/O failure drops `tx` (rolling the row back) and removes what was created.
        let created = {
            let (instance_dir, folder, meta) =
                (instance_dir.clone(), folder_path.clone(), meta_path.clone());
            tokio::task::spawn_blocking(move || create_dirs(&instance_dir, &folder, &meta))
                .await
                .map_err(|_| AfmError::io())?
        };
        if let Err(failure) = created {
            return Err(match failure {
                CreateFailure::NotEmpty => AfmError::FolderExists { id },
                CreateFailure::Io => AfmError::io(),
            });
        }
        tx.commit().await.map_err(failed("commit_create"))?;
        self.record(
            "agent_folder_created",
            None,
            None,
            "Agent folder created.",
            serde_json::json!({ "agent_instance_id": id }),
        )
        .await;
        crate::log::created(id, elapsed_ms(started));
        Ok(AgentFolder {
            agent_instance_id: id,
            folder_path,
            meta_path,
            state: AgentFolderState::Created,
            created_at,
        })
    }

    pub async fn get(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError> {
        let row = self.load_created(id).await?;
        Ok(AgentFolder {
            agent_instance_id: id,
            folder_path: row.folder_path,
            meta_path: row.meta_path,
            state: row.state,
            created_at: row.created_at,
        })
    }

    /// Step 4. Unknown instance or state not `created` is `AFM_FOLDER_NOT_FOUND`.
    pub async fn lock(&self, id: AgentInstanceId, mode: LockMode) -> Result<FolderLease, AfmError> {
        let row = self.load_created(id).await?;
        let cell = Arc::clone(guard(&self.locks).entry(id).or_default());
        acquire(
            cell,
            id,
            mode,
            row.folder_path,
            row.meta_path,
            self.lock_wait,
        )
        .await
    }

    /// Step 5. Deletes the agent folder and its meta folder; refused while any lease is held.
    pub async fn remove(&self, id: AgentInstanceId) -> Result<(), AfmError> {
        let started = std::time::Instant::now();
        let mut lease = self.lock(id, LockMode::Write).await?;
        let _gate = self.write_gate.lock().await;
        let (folder, meta) = (lease.folder().to_path_buf(), lease.meta().to_path_buf());
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_remove"))?;
        tx.execute(
            "UPDATE agent_folders SET state = 'removing' WHERE agent_instance_id = $1{uuid}",
            &[Arg::text(id)],
        )
        .await
        .map_err(failed("mark_removing"))?;
        tx.execute(
            "DELETE FROM session_folder_access WHERE agent_instance_id = $1{uuid}",
            &[Arg::text(id)],
        )
        .await
        .map_err(failed("delete_session_access"))?;
        tx.commit().await.map_err(failed("commit_removing"))?;
        guard(&self.live).retain(|_, session| session.agent_instance_id != id);
        // The lock file is deleted with `meta/`, so the file lock goes first.
        lease.release_os_lock();
        delete_instance_dirs(folder, meta).await?;
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_removed"))?;
        tx.execute(
            "UPDATE agent_folders SET state = 'removed', removed_at = $2{ts} \
             WHERE agent_instance_id = $1{uuid}",
            &[Arg::text(id), Arg::text(Timestamp::now().to_wire())],
        )
        .await
        .map_err(failed("mark_removed"))?;
        tx.commit().await.map_err(failed("commit_removed"))?;
        self.record(
            "agent_folder_removed",
            None,
            None,
            "Agent folder removed.",
            serde_json::json!({ "agent_instance_id": id }),
        )
        .await;
        crate::log::removed(id, elapsed_ms(started));
        Ok(())
    }

    /// The row of an instance in state `created`, else `AFM_FOLDER_NOT_FOUND`.
    pub(crate) async fn load_created(&self, id: AgentInstanceId) -> Result<FolderRow, AfmError> {
        let row = db::fetch_optional(&self.store, SELECT_ROW, &[Arg::text(id)])
            .await
            .map_err(failed("select_agent_folder"))?;
        match row.as_ref().map(FolderRow::parse).transpose()? {
            Some(row) if row.state == AgentFolderState::Created => Ok(row),
            _ => Err(AfmError::FolderNotFound { id }),
        }
    }

    /// Appends one of the four journal entry kinds. A journal failure is logged at `error` with
    /// the `JRNL_*` code and does not undo the operation it records (SA-RECON-03).
    // spec: ordin-desktop/components/AgentFolderManager#SA-RECON-03
    pub(crate) async fn record(
        &self,
        kind: &'static str,
        run_id: Option<RunId>,
        workspace_id: Option<WorkspaceId>,
        summary: &str,
        data: serde_json::Value,
    ) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: MODULE.to_owned(),
            run_id,
            crew_run_id: None,
            workspace_id,
            summary: summary.to_owned(),
            data,
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            crate::log::journal_failed(kind, error.code());
        }
    }
}

/// Deletes `folder/`, `meta/` and the instance directory above them, never following links.
async fn delete_instance_dirs(folder: PathBuf, meta: PathBuf) -> Result<(), AfmError> {
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        remove_tree(&folder)?;
        remove_tree(&meta)?;
        if let Some(instance_dir) = folder.parent() {
            match std::fs::remove_dir(instance_dir) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| AfmError::io())?
    .map_err(|_| AfmError::io())
}

enum CreateFailure {
    /// `folder/` exists and is not an empty directory.
    NotEmpty,
    Io,
}

/// Step 3's directories (mode `0700`). `folder/` must be empty or absent. On a failure the
/// directories this call made are removed again.
fn create_dirs(instance_dir: &Path, folder: &Path, meta: &Path) -> Result<(), CreateFailure> {
    match std::fs::symlink_metadata(folder) {
        Ok(found) => {
            let empty = found.file_type().is_dir()
                && std::fs::read_dir(folder)
                    .map_err(|_| CreateFailure::Io)?
                    .next()
                    .is_none();
            if !empty {
                return Err(CreateFailure::NotEmpty);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(CreateFailure::Io),
    }
    let mut made: Vec<&Path> = Vec::new();
    let result = [instance_dir, folder, meta]
        .into_iter()
        .try_for_each(|dir| {
            if dir.is_dir() {
                return Ok(());
            }
            make_private_dir_all(dir)?;
            made.push(dir);
            Ok::<(), std::io::Error>(())
        });
    if result.is_err() {
        for dir in made.into_iter().rev() {
            let _ = remove_tree(dir);
        }
        return Err(CreateFailure::Io);
    }
    Ok(())
}
