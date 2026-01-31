//! `Installers`: install, update, verify, repair and uninstall an agent folder (Installers.md
//! *Internal Logic* steps 8 to 17), and the release-bundle writer (step 14b).
//!
//! Every write happens under a lease of the Agent Folder Manager; the install record, the trusted
//! lock-file copy and the backups live in the instance's `meta/` folder, out of a session's reach.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use ordin_agent_folders::{AgentFolderManager, FolderLease, LockMode};
use ordin_agent_model::{LockFile, SourceScope, BUNDLE_MANIFEST_NAME, LOCK_FILE_NAME};
use ordin_resolver::{Bundle, ResolverAndCompiler};
use ordin_types::time::Timestamp;
use ordin_types::AgentInstanceId;

use crate::error::{bundle_unavailable, folder_unavailable, InstallError};
use crate::fsio::{
    empty_dir, has_entries, join, make_private_dir, on_disk, prune_empty_parents, read_record,
    remove_entry, remove_tree, sha256_hex, write_atomic, write_record, OnDisk,
};
use crate::log::{self, Counts, PathEvent};
use crate::render::{self, check_path, Rendered};
use crate::types::{
    installer_for, ClaudeSessionFormInput, FileAction, HarnessLaunch, InstallOutcome,
    InstallRecord, InstallRequest, InstalledFile, InstallerKind, RenderedTree, VerifyOutcome,
    BACKUPS_DIR, BUNDLE_COPY_DIR, RECORD_FILE, RECORD_SCHEMA,
};

pub struct Installers {
    folders: Arc<AgentFolderManager>,
    resolver: Arc<ResolverAndCompiler>,
}

/// Step 8's result before the lease is taken.
struct Prepared {
    kind: InstallerKind,
    bundle: Bundle,
    manifest_bytes: Vec<u8>,
    rendered: Rendered,
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn io<T>(result: std::io::Result<T>) -> Result<T, InstallError> {
    result.map_err(|_| InstallError::io())
}

impl Installers {
    pub fn new(folders: Arc<AgentFolderManager>, resolver: Arc<ResolverAndCompiler>) -> Self {
        Installers { folders, resolver }
    }

    /// Step 8, before the lease: the installer, the bundle and the render.
    fn prepare(&self, request: &InstallRequest) -> Result<Prepared, InstallError> {
        let kind = installer_for(request.harness)?;
        let bundle = self
            .resolver
            .open_bundle(&request.plan_hash)
            .map_err(|e| bundle_unavailable(&e))?;
        let manifest_bytes = fs::read(bundle.dir.join(BUNDLE_MANIFEST_NAME)).map_err(|_| {
            InstallError::BundleUnavailable {
                code: "RES_BUNDLE_CORRUPT".to_owned(),
                retryable: false,
            }
        })?;
        let rendered = render::render(kind, &bundle, Some(request.agent_instance_id))?;
        Ok(Prepared {
            kind,
            bundle,
            manifest_bytes,
            rendered,
        })
    }

    async fn lease(
        &self,
        id: AgentInstanceId,
        mode: LockMode,
    ) -> Result<FolderLease, InstallError> {
        self.folders
            .lock(id, mode)
            .await
            .map_err(|e| folder_unavailable(&e))
    }

    /// First install into an existing, empty agent folder (created by the caller, 03 *Add an agent*).
    // spec: ordin-desktop/components/Installers#SA-RECON-03
    pub async fn install(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        let started = Instant::now();
        let id = request.agent_instance_id;
        let prepared = self.prepare(request)?;
        let lease = self.lease(id, LockMode::Write).await?;
        let (folder, meta) = (lease.folder(), lease.meta());
        // Step 9: the folder must be empty and hold no record.
        if io(has_entries(folder))? || meta.join(RECORD_FILE).exists() {
            return Err(InstallError::FolderNotEmpty);
        }
        let mut files = Vec::with_capacity(prepared.rendered.tree.files.len());
        let mut counts = Counts::default();
        for (path, bytes) in &prepared.rendered.tree.files {
            if write_atomic(folder, path, bytes).is_err() {
                // Step 11: a failed first install leaves the folder empty.
                let _ = empty_dir(folder);
                return Err(InstallError::io());
            }
            files.push(installed(path, bytes, FileAction::Created, None, None));
            counts.created += 1;
        }
        let record = build_record(request, &prepared, folder, files, None);
        if let Err(error) = write_meta(meta, &prepared, &record) {
            let _ = empty_dir(folder);
            let _ = remove_tree(&meta.join(BUNDLE_COPY_DIR));
            return Err(error);
        }
        log::written(
            false,
            id,
            prepared.kind,
            &request.plan_hash,
            counts,
            elapsed_ms(started),
        );
        Ok(InstallOutcome {
            record,
            restart_required: false,
            repaired: false,
        })
    }

    /// Replace the installed bundle with another between runs (Write lease).
    pub async fn update(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError> {
        self.update_with(request, LockMode::Write).await
    }

    /// Pack-choice rebuild while a session runs (SessionUpdate lease, ADR-089 decision 64, C-64).
    // spec: ordin-desktop/components/Installers#C-64
    pub async fn update_in_session(
        &self,
        request: &InstallRequest,
    ) -> Result<InstallOutcome, InstallError> {
        self.update_with(request, LockMode::SessionUpdate).await
    }

    /// Steps 10 to 14.
    async fn update_with(
        &self,
        request: &InstallRequest,
        mode: LockMode,
    ) -> Result<InstallOutcome, InstallError> {
        let started = Instant::now();
        let id = request.agent_instance_id;
        let prepared = self.prepare(request)?;
        let lease = self.lease(id, mode).await?;
        let (folder, meta) = (lease.folder(), lease.meta());
        let old = read_record(meta)?;
        let old_by_path: BTreeMap<&str, &InstalledFile> =
            old.files.iter().map(|f| (f.path.as_str(), f)).collect();
        let mut backups = Backups::new(meta);
        let mut counts = Counts::default();
        let mut files = Vec::with_capacity(prepared.rendered.tree.files.len());
        for (path, bytes) in &prepared.rendered.tree.files {
            let digest = sha256_hex(bytes);
            let disk = io(on_disk(folder, path))?;
            let entry = match (old_by_path.get(path.as_str()), disk) {
                // Recorded and untouched on disk: write only what differs.
                (Some(old_entry), OnDisk::File(on)) if on == old_entry.sha256 => {
                    let action = if on == digest {
                        FileAction::Unchanged
                    } else {
                        io(write_atomic(folder, path, bytes))?;
                        FileAction::Replaced
                    };
                    carried(path, bytes, action, old_entry)
                }
                // Recorded, then changed by the session or the person: back it up first.
                (Some(old_entry), OnDisk::File(on)) => {
                    let backup = backups.keep(folder, path)?;
                    io(write_atomic(folder, path, bytes))?;
                    // The earliest backup is the one uninstall restores.
                    match (&old_entry.pre_sha256, &old_entry.backup) {
                        (Some(_), Some(_)) => carried(path, bytes, FileAction::Replaced, old_entry),
                        _ => installed(path, bytes, FileAction::Replaced, Some(on), Some(backup)),
                    }
                }
                // Recorded, but gone or no longer a regular file: rewritten.
                (Some(old_entry), _) => {
                    io(write_atomic(folder, path, bytes))?;
                    carried(path, bytes, FileAction::Replaced, old_entry)
                }
                // New to the record, over a file a session wrote: back it up first.
                (None, OnDisk::File(on)) => {
                    let backup = backups.keep(folder, path)?;
                    io(write_atomic(folder, path, bytes))?;
                    installed(path, bytes, FileAction::Replaced, Some(on), Some(backup))
                }
                (None, OnDisk::Other) => {
                    io(write_atomic(folder, path, bytes))?;
                    installed(path, bytes, FileAction::Replaced, None, None)
                }
                (None, OnDisk::Missing) => {
                    io(write_atomic(folder, path, bytes))?;
                    installed(path, bytes, FileAction::Created, None, None)
                }
            };
            match entry.action {
                FileAction::Created => counts.created += 1,
                FileAction::Replaced => counts.replaced += 1,
                FileAction::Unchanged => counts.unchanged += 1,
            }
            files.push(entry);
        }
        // Recorded paths the new bundle no longer holds.
        let mut removed_paths = Vec::new();
        for old_entry in &old.files {
            if prepared.rendered.tree.files.contains_key(&old_entry.path) {
                continue;
            }
            match io(on_disk(folder, &old_entry.path))? {
                OnDisk::File(on) if on == old_entry.sha256 => {
                    io(remove_entry(&join(folder, &old_entry.path)))?;
                    counts.removed += 1;
                }
                OnDisk::File(_) => {
                    backups.keep(folder, &old_entry.path)?;
                    io(remove_entry(&join(folder, &old_entry.path)))?;
                    log::path_warning(PathEvent::KeptChangedFile, Some(id), &old_entry.path);
                }
                OnDisk::Missing | OnDisk::Other => {}
            }
            // `InstallRecord` holds only installed files, so the earliest backup of a path the new
            // bundle drops is restored now, which leaves the folder uninstall would have left.
            if let Some(backup) = &old_entry.backup {
                if let Ok(original) = fs::read(join(meta, backup)) {
                    io(write_atomic(folder, &old_entry.path, &original))?;
                }
            }
            prune_empty_parents(folder, &old_entry.path);
            removed_paths.push(old_entry.path.clone());
        }
        counts.backed_up = backups.count;
        let record = build_record(request, &prepared, folder, files, Some(&old));
        write_meta(meta, &prepared, &record)?;
        let restart_required =
            mode == LockMode::SessionUpdate && restart_needed(&record, &removed_paths);
        log::written(
            true,
            id,
            prepared.kind,
            &request.plan_hash,
            counts,
            elapsed_ms(started),
        );
        Ok(InstallOutcome {
            record,
            restart_required,
            repaired: false,
        })
    }

    /// Compare the folder with the install record and the lock file (SA-INST-01).
    pub async fn verify(&self, id: AgentInstanceId) -> Result<VerifyOutcome, InstallError> {
        Ok(self.verify_inner(id).await?.0)
    }

    /// Step 15, read-only, under a `Session` lease for the duration of the check.
    // spec: ordin-desktop/components/Installers#SA-INST-01
    async fn verify_inner(
        &self,
        id: AgentInstanceId,
    ) -> Result<(VerifyOutcome, InstallRecord), InstallError> {
        let lease = self.lease(id, LockMode::Session).await?;
        let (folder, meta) = (lease.folder(), lease.meta());
        let record = read_record(meta)?;
        let mut changed = Vec::new();
        let mut missing = Vec::new();
        for file in &record.files {
            match io(on_disk(folder, &file.path))? {
                OnDisk::Missing => missing.push(file.path.clone()),
                OnDisk::File(on) if on == file.sha256 => {}
                OnDisk::File(_) | OnDisk::Other => changed.push(file.path.clone()),
            }
        }
        match io(on_disk(folder, LOCK_FILE_NAME))? {
            OnDisk::File(on) if on == record.lock_file_sha256 => {}
            OnDisk::Missing => {
                if !missing.iter().any(|p| p == LOCK_FILE_NAME) {
                    missing.push(LOCK_FILE_NAME.to_owned());
                }
            }
            _ => {
                if !changed.iter().any(|p| p == LOCK_FILE_NAME) {
                    changed.push(LOCK_FILE_NAME.to_owned());
                }
            }
        }
        changed.sort();
        missing.sort();
        log::verified(id, changed.len(), missing.len());
        let outcome = if changed.is_empty() && missing.is_empty() {
            VerifyOutcome::Verified
        } else {
            VerifyOutcome::Mismatch { changed, missing }
        };
        Ok((outcome, record))
    }

    /// Verify; on a mismatch rebuild from the lock file without asking (SA-INST-01).
    // spec: ordin-desktop/components/Installers#SA-INST-01
    pub async fn verify_and_repair(
        &self,
        id: AgentInstanceId,
        scope: &SourceScope,
    ) -> Result<InstallOutcome, InstallError> {
        let (outcome, record) = self.verify_inner(id).await?;
        let VerifyOutcome::Mismatch { changed, missing } = outcome else {
            return Ok(InstallOutcome {
                record,
                restart_required: false,
                repaired: false,
            });
        };
        match self.repair(id, scope, changed.len(), missing.len()).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                log::repair_failed(id, error.code());
                Err(error)
            }
        }
    }

    /// Step 16.
    async fn repair(
        &self,
        id: AgentInstanceId,
        scope: &SourceScope,
        changed: usize,
        missing: usize,
    ) -> Result<InstallOutcome, InstallError> {
        let lease = self.lease(id, LockMode::Write).await?;
        let (folder, meta) = (lease.folder(), lease.meta());
        let mut record = read_record(meta)?;
        let lock_invalid = || InstallError::RepairFailed {
            code: "AM_LOCK_FILE_INVALID".to_owned(),
        };
        let lock_bytes = fs::read(join(meta, &format!("{BUNDLE_COPY_DIR}/{LOCK_FILE_NAME}")))
            .map_err(|_| lock_invalid())?;
        let lock = LockFile::from_yaml(&lock_bytes).map_err(|_| lock_invalid())?;
        let bundle = match self.resolver.open_bundle(&lock.plan_hash) {
            Ok(bundle) => bundle,
            Err(_) => self
                .resolver
                .rebuild_from_lock(&lock, scope)
                .await
                .map_err(|e| InstallError::RepairFailed {
                    code: e.code().to_owned(),
                })?,
        };
        let rendered = render::render(record.installer, &bundle, Some(id)).map_err(|e| {
            InstallError::RepairFailed {
                code: e.code().to_owned(),
            }
        })?;
        let mut rewritten = 0;
        for file in &mut record.files {
            let Some(bytes) = rendered.tree.files.get(&file.path) else {
                continue;
            };
            let intact =
                matches!(io(on_disk(folder, &file.path))?, OnDisk::File(on) if on == file.sha256);
            if !intact {
                io(write_atomic(folder, &file.path, bytes))?;
                file.sha256 = sha256_hex(bytes);
                file.size = bytes.len() as u64;
                file.action = FileAction::Replaced;
                rewritten += 1;
            }
        }
        record.updated_at = Timestamp::now();
        write_record(meta, &record)?;
        log::repaired(id, changed, missing, rewritten);
        Ok(InstallOutcome {
            record,
            restart_required: false,
            repaired: true,
        })
    }

    /// Restore the folder to its state before the first install (Write lease, step 17).
    pub async fn uninstall(&self, id: AgentInstanceId) -> Result<(), InstallError> {
        let lease = self.lease(id, LockMode::Write).await?;
        let (folder, meta) = (lease.folder(), lease.meta());
        let record = read_record(meta)?;
        for file in &record.files {
            match &file.backup {
                Some(backup) => match fs::read(join(meta, backup)) {
                    Ok(original) => {
                        if file.pre_sha256.as_deref() != Some(sha256_hex(&original).as_str()) {
                            log::path_warning(PathEvent::BackupMismatch, Some(id), &file.path);
                        }
                        io(write_atomic(folder, &file.path, &original))?;
                    }
                    Err(_) => {
                        log::path_warning(PathEvent::BackupMismatch, Some(id), &file.path);
                        remove_installed(folder, &file.path)?;
                    }
                },
                None => remove_installed(folder, &file.path)?,
            }
        }
        for file in &record.files {
            prune_empty_parents(folder, &file.path);
        }
        io(remove_entry(&meta.join(RECORD_FILE)))?;
        io(remove_tree(&meta.join(BUNDLE_COPY_DIR)))?;
        io(remove_tree(&meta.join(BACKUPS_DIR)))?;
        Ok(())
    }

    /// The install record; read by the Package Catalogue and the program's `SessionSetupSource`.
    // spec: ordin-desktop/components/Installers#SA-RECON-02
    pub async fn record(&self, id: AgentInstanceId) -> Result<InstallRecord, InstallError> {
        let folder = self
            .folders
            .get(id)
            .await
            .map_err(|e| folder_unavailable(&e))?;
        read_record(&folder.meta_path)
    }

    /// Pure render, for golden-tree tests (no folder touched).
    pub fn render(
        &self,
        kind: InstallerKind,
        bundle: &Bundle,
    ) -> Result<RenderedTree, InstallError> {
        render::render(kind, bundle, None).map(|r| r.tree)
    }
}

/// A file a recorded path holds is deleted; a directory there is not ours and is left alone.
fn remove_installed(folder: &Path, path: &str) -> Result<(), InstallError> {
    match io(on_disk(folder, path))? {
        OnDisk::Missing => Ok(()),
        OnDisk::File(_) => io(remove_entry(&join(folder, path))),
        OnDisk::Other => {
            let full = join(folder, path);
            if fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink()) {
                io(remove_entry(&full))
            } else {
                Ok(())
            }
        }
    }
}

/// A record entry for bytes just written (or found equal).
fn installed(
    path: &str,
    bytes: &[u8],
    action: FileAction,
    pre_sha256: Option<String>,
    backup: Option<String>,
) -> InstalledFile {
    InstalledFile {
        path: path.to_owned(),
        sha256: sha256_hex(bytes),
        size: bytes.len() as u64,
        action,
        pre_sha256,
        backup,
    }
}

/// A record entry that keeps the earlier entry's `pre_sha256` and `backup`.
fn carried(path: &str, bytes: &[u8], action: FileAction, old: &InstalledFile) -> InstalledFile {
    installed(
        path,
        bytes,
        action,
        old.pre_sha256.clone(),
        old.backup.clone(),
    )
}

/// Backups of one update, under `meta/backups/<UTC yyyymmddThhmmssZ>/`.
struct Backups<'a> {
    meta: &'a Path,
    stamp: Option<String>,
    count: u64,
}

impl<'a> Backups<'a> {
    fn new(meta: &'a Path) -> Self {
        Backups {
            meta,
            stamp: None,
            count: 0,
        }
    }

    /// The stamp of this update, chosen on the first backup.
    // Two updates within one second would share `backups/<stamp>/` and the second could
    // overwrite a backup the record still names; a stamp already taken gets a `-<n>` suffix.
    fn stamp(&mut self) -> String {
        if let Some(stamp) = &self.stamp {
            return stamp.clone();
        }
        let base = Timestamp::now().0.format("%Y%m%dT%H%M%SZ").to_string();
        let mut stamp = base.clone();
        let mut n = 1;
        while self.meta.join(BACKUPS_DIR).join(&stamp).exists() {
            stamp = format!("{base}-{n}");
            n += 1;
        }
        self.stamp = Some(stamp.clone());
        stamp
    }

    /// Copies the file at `path` to `backups/<stamp>/<path>`; returns that path relative to `meta/`.
    fn keep(&mut self, folder: &Path, path: &str) -> Result<String, InstallError> {
        let bytes = io(fs::read(join(folder, path)))?;
        let relative = format!("{BACKUPS_DIR}/{}/{path}", self.stamp());
        io(write_atomic(self.meta, &relative, &bytes))?;
        self.count += 1;
        Ok(relative)
    }
}

/// Step 12: the record of what was just written.
fn build_record(
    request: &InstallRequest,
    prepared: &Prepared,
    folder: &Path,
    files: Vec<InstalledFile>,
    previous: Option<&InstallRecord>,
) -> InstallRecord {
    let now = Timestamp::now();
    let tree = &prepared.rendered.tree;
    let launch = match &tree.launch_paths {
        Some((instructions, plugin, name)) => HarnessLaunch::ClaudeCode {
            working_dir: folder.to_path_buf(),
            append_system_prompt_file: join(folder, instructions),
            plugin_dir: join(folder, plugin),
            plugin_name: name.clone(),
        },
        None => HarnessLaunch::Copilot {
            working_dir: folder.to_path_buf(),
        },
    };
    InstallRecord {
        record_schema: RECORD_SCHEMA.to_owned(),
        agent_instance_id: request.agent_instance_id,
        installer: prepared.kind,
        harness: request.harness,
        plan_hash: request.plan_hash.clone(),
        previous_plan_hash: previous.map(|r| r.plan_hash.clone()),
        lock_file_sha256: sha256_hex(&prepared.bundle.lock_file_bytes),
        tool_policy: prepared.bundle.manifest.resolved_tool_policy.clone(),
        launch,
        commands: tree.commands.clone(),
        needs_person_present: prepared.rendered.needs_person_present,
        reference_only_parts: tree.reference_only_parts.clone(),
        files,
        installed_at: previous.map(|r| r.installed_at).unwrap_or(now),
        updated_at: now,
    }
}

/// Step 12: `meta/bundle/` and the record.
fn write_meta(
    meta: &Path,
    prepared: &Prepared,
    record: &InstallRecord,
) -> Result<(), InstallError> {
    io(write_atomic(
        meta,
        &format!("{BUNDLE_COPY_DIR}/{BUNDLE_MANIFEST_NAME}"),
        &prepared.manifest_bytes,
    ))?;
    io(write_atomic(
        meta,
        &format!("{BUNDLE_COPY_DIR}/{LOCK_FILE_NAME}"),
        &prepared.bundle.lock_file_bytes,
    ))?;
    write_record(meta, record)
}

/// Step 13: Claude Code reads `instructions.md` and `plugin/` only at start (C-61); Copilot's
/// session must be resumed until C-64 confirms otherwise.
fn restart_needed(record: &InstallRecord, removed: &[String]) -> bool {
    let launch_file =
        |path: &str| path == crate::claude::INSTRUCTIONS || path.starts_with("plugin/");
    match record.installer {
        InstallerKind::ClaudeCode => {
            record
                .files
                .iter()
                .any(|f| f.action != FileAction::Unchanged && launch_file(&f.path))
                || removed.iter().any(|p| launch_file(p))
        }
        InstallerKind::Vscode | InstallerKind::AgentsMd => true,
    }
}

/// Pure: `instructions.md` and `plugin/` for a release bundle (Release Builder P-9).
// spec: ordin-desktop/components/Installers#SA-RECON-04
pub fn render_claude_session_form(
    input: &ClaudeSessionFormInput,
    bundle_dir: &Path,
) -> Result<RenderedTree, InstallError> {
    crate::claude::session_form(input, bundle_dir)
}

/// Writes a rendered tree into `target_dir` with step 11's atomic file writes; refuses a target that
/// already holds any of the tree's paths. Takes no lease: the caller owns `target_dir`.
// spec: ordin-desktop/components/Installers#SA-RECON-04
pub async fn write_tree(target_dir: &Path, tree: &RenderedTree) -> Result<(), InstallError> {
    create_private_all(target_dir)?;
    for path in tree.files.keys() {
        check_path(path)?;
        if fs::symlink_metadata(join(target_dir, path)).is_ok() {
            return Err(InstallError::FolderNotEmpty);
        }
    }
    let mut written: Vec<&str> = Vec::with_capacity(tree.files.len());
    for (path, bytes) in &tree.files {
        if write_atomic(target_dir, path, bytes).is_err() {
            for done in written {
                let _ = remove_entry(&join(target_dir, done));
                prune_empty_parents(target_dir, done);
            }
            return Err(InstallError::io());
        }
        written.push(path);
    }
    Ok(())
}

/// `target_dir` and its missing parents, mode `0700`.
fn create_private_all(dir: &Path) -> Result<(), InstallError> {
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        if !parent.as_os_str().is_empty() {
            create_private_all(parent)?;
        }
    }
    io(make_private_dir(dir))
}
