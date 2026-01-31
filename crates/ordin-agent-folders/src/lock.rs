//! Folder leases: an in-process reader–writer table plus an operating-system file lock on
//! `meta/folder.lock` (AgentFolderManager.md Internal Logic 4).

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ordin_types::AgentInstanceId;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::error::AfmError;
use crate::types::LockMode;

/// How long a `Write` or `SessionUpdate` request waits for the folder (*assumed here*: longer than
/// one file copy of a 200-part bundle, shorter than a person's patience at **Add it**).
pub const WRITE_LOCK_WAIT: Duration = Duration::from_secs(30);

/// The OS lock is retried at this interval within the same wait (step 4).
const OS_LOCK_RETRY: Duration = Duration::from_millis(100);

/// What is held on one instance.
#[derive(Debug, Default)]
struct LockState {
    sessions: u32,
    write: bool,
    session_update: bool,
}

impl LockState {
    fn grantable(&self, mode: LockMode) -> bool {
        match mode {
            LockMode::Session => !self.write,
            LockMode::SessionUpdate => !self.write && !self.session_update,
            LockMode::Write => self.sessions == 0 && !self.write && !self.session_update,
        }
    }

    /// The name of the lease that blocks `mode`, for `AFM_FOLDER_BUSY { held }`.
    fn blocker(&self, mode: LockMode) -> &'static str {
        if self.write {
            "write"
        } else if self.session_update && mode != LockMode::Session {
            "session_update"
        } else {
            "session"
        }
    }

    fn take(&mut self, mode: LockMode) {
        match mode {
            LockMode::Session => self.sessions += 1,
            LockMode::Write => self.write = true,
            LockMode::SessionUpdate => self.session_update = true,
        }
    }

    fn release(&mut self, mode: LockMode) {
        match mode {
            LockMode::Session => self.sessions = self.sessions.saturating_sub(1),
            LockMode::Write => self.write = false,
            LockMode::SessionUpdate => self.session_update = false,
        }
    }
}

/// The in-process state of one instance. A standard mutex, because `FolderLease::drop` is
/// synchronous; it is never held across an await.
#[derive(Debug, Default)]
pub(crate) struct InstanceLock {
    state: Mutex<LockState>,
    notify: Notify,
}

impl InstanceLock {
    fn with_state<T>(&self, f: impl FnOnce(&mut LockState) -> T) -> T {
        f(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Held for as long as the lock is needed; dropping it releases the lock.
pub struct FolderLease {
    id: AgentInstanceId,
    mode: LockMode,
    folder: PathBuf,
    meta: PathBuf,
    file: Option<File>,
    cell: Arc<InstanceLock>,
}

impl FolderLease {
    pub fn mode(&self) -> LockMode {
        self.mode
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn meta(&self) -> &Path {
        &self.meta
    }

    /// Lets go of the file lock early, so the file can be deleted on every platform; the
    /// in-process grant stays until the lease is dropped.
    pub(crate) fn release_os_lock(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = file.unlock();
        }
    }
}

impl std::fmt::Debug for FolderLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FolderLease")
            .field("id", &self.id)
            .field("mode", &self.mode)
            .finish()
    }
}

impl Drop for FolderLease {
    fn drop(&mut self) {
        self.release_os_lock();
        let mode = self.mode;
        self.cell.with_state(|state| state.release(mode));
        self.cell.notify.notify_waiters();
    }
}

/// Step 4: the in-process grant, then the OS lock, within one `wait`.
pub(crate) async fn acquire(
    cell: Arc<InstanceLock>,
    id: AgentInstanceId,
    mode: LockMode,
    folder: PathBuf,
    meta: PathBuf,
    wait: Duration,
) -> Result<FolderLease, AfmError> {
    let deadline = Instant::now() + wait;
    let busy = |held: &'static str| {
        crate::log::lock_busy(id, mode.label(), held);
        AfmError::FolderBusy { id, held }
    };
    // In-process grant. A `Session` request never waits: the Session Supervisor reports the agent busy.
    loop {
        let notified = cell.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let outcome = cell.with_state(|state| {
            if state.grantable(mode) {
                state.take(mode);
                Ok(())
            } else {
                Err(state.blocker(mode))
            }
        });
        let held = match outcome {
            Ok(()) => break,
            Err(held) => held,
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if mode == LockMode::Session || remaining.is_zero() {
            return Err(busy(held));
        }
        let _ = tokio::time::timeout(remaining, notified).await;
    }
    // From here the lease releases the in-process grant if anything below fails.
    let mut lease = FolderLease {
        id,
        mode,
        folder,
        meta,
        file: None,
        cell,
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lease.meta.join("folder.lock"))
        .map_err(|_| AfmError::io())?;
    loop {
        let attempt = match mode {
            LockMode::Write => file.try_lock(),
            LockMode::Session | LockMode::SessionUpdate => file.try_lock_shared(),
        };
        match attempt {
            Ok(()) => break,
            Err(TryLockError::Error(_)) => return Err(AfmError::io()),
            Err(TryLockError::WouldBlock) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if mode == LockMode::Session || remaining.is_zero() {
                    return Err(busy("another ordin process"));
                }
                tokio::time::sleep(remaining.min(OS_LOCK_RETRY)).await;
            }
        }
    }
    lease.file = Some(file);
    Ok(lease)
}
