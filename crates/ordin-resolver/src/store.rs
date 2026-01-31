//! Bundles on disk: compile, open, read and prune (ResolverAndCompiler.md *Internal Logic* 16 to 21 and
//! 25, *Data Model*).
//!
//! ```text
//! <bundle_root>/
//!   <plan hash hex>/                    one bundle, immutable once renamed into place
//!     bundle.manifest.json              canonical JSON (BundleManifest)
//!     ordin.lock                        canonical YAML (LockFile)
//!     artefacts/<content hash hex>/     the part's files, as committed
//!   .tmp-<UUIDv7>/                      staging
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ordin_agent_model::{
    canonical_json, content_digest, BundleArtefact, BundleManifest, LockFile, PartFiles, PartRef,
    PartSource, ResolutionPlan, BUNDLE_MANIFEST_NAME, BUNDLE_SCHEMA, LOCK_FILE_NAME,
};

use crate::error::{store_failed, ResolveError};
use crate::held::{files_of, Held};
use crate::log;
use crate::request::PruneReport;
use crate::resolve::{loading_of, lock_of, policy_of, sha256_hex};

const ARTEFACTS_DIR: &str = "artefacts";
const STAGING_PREFIX: &str = ".tmp-";
const SHA256_PREFIX: &str = "sha256:";
/// A staging directory older than this belongs to a compile that did not finish.
const STALE_STAGING: Duration = Duration::from_secs(60 * 60);

/// A bundle on disk: `<bundle_root>/<plan hash hex>/`.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub plan: ResolutionPlan,
    pub manifest: BundleManifest,
    pub lock_file: LockFile,
    /// The exact bytes of `ordin.lock` (canonical YAML).
    pub lock_file_bytes: Vec<u8>,
    pub dir: PathBuf,
}

impl Bundle {
    /// `<dir>/artefacts/<content hash hex>/`.
    pub fn part_dir(&self, part: &PartRef) -> PathBuf {
        self.dir
            .join(ARTEFACTS_DIR)
            .join(hex_of(&part.content_hash))
    }

    /// The files of one part, read from the bundle and checked against the part's digest.
    pub fn read_part_files(&self, part: &PartRef) -> Result<PartFiles, ResolveError> {
        let corrupt = || ResolveError::BundleCorrupt {
            plan_hash: self.plan.plan_hash.clone(),
        };
        let files = read_files(&self.part_dir(part)).ok_or_else(corrupt)?;
        if format!("{SHA256_PREFIX}{}", content_digest(&files).0) != part.content_hash {
            return Err(corrupt());
        }
        Ok(files)
    }
}

/// The hexadecimal part of `sha256:<hex>`.
fn hex_of(prefixed: &str) -> &str {
    prefixed.strip_prefix(SHA256_PREFIX).unwrap_or(prefixed)
}

fn is_hex_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Every file under `dir`, keyed by its `/`-separated relative path; None when `dir` cannot be read.
fn read_files(dir: &Path) -> Option<PartFiles> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Option<()> {
        for entry in fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                walk(base, &path, out)?;
            } else {
                let relative = path
                    .strip_prefix(base)
                    .ok()?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(relative, fs::read(&path).ok()?);
            }
        }
        Some(())
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out)?;
    Some(PartFiles(out))
}

/// Step 17: a relative name that is absolute or holds `..`, a backslash or a NUL.
fn path_unsafe(name: &str) -> bool {
    name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.split('/').any(|segment| segment == "..")
}

/// Writes `bytes` to `path`, creating parent directories, and flushes them to disk.
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), ResolveError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| store_failed(&e))?;
    }
    let mut file = fs::File::create(path).map_err(|e| store_failed(&e))?;
    file.write_all(bytes).map_err(|e| store_failed(&e))?;
    file.sync_all().map_err(|e| store_failed(&e))
}

/// Removes the staging directory when a compile ends without publishing it.
struct Staging {
    dir: PathBuf,
    published: bool,
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

fn bundle_dir(root: &Path, plan_hash: &str) -> PathBuf {
    root.join(hex_of(plan_hash))
}

/// Steps 16 to 20. `held` is aligned with `plan.resolved`.
pub(crate) async fn compile_held(
    root: &Path,
    compiler_version: &str,
    source: &dyn PartSource,
    plan: &ResolutionPlan,
    held: &[Held],
) -> Result<Bundle, ResolveError> {
    let started = Instant::now();
    let final_dir = bundle_dir(root, &plan.plan_hash);
    // Step 16: reuse a bundle that is already whole.
    if let Ok(mut existing) = open_bundle(root, &plan.plan_hash) {
        existing.plan = plan.clone();
        log::compiled(
            &plan.plan_hash,
            plan.resolved.len(),
            0,
            true,
            started.elapsed(),
        );
        return Ok(existing);
    }
    // Step 17: stage.
    fs::create_dir_all(root).map_err(|e| store_failed(&e))?;
    let staging_dir = root.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::now_v7()));
    fs::create_dir(&staging_dir).map_err(|e| store_failed(&e))?;
    let mut staging = Staging {
        dir: staging_dir.clone(),
        published: false,
    };
    let mut written: BTreeSet<String> = BTreeSet::new();
    let mut bytes_total: u64 = 0;
    for (part_ref, part) in plan.resolved.iter().zip(held) {
        let hex = hex_of(&part_ref.content_hash).to_owned();
        if written.contains(&hex) {
            continue;
        }
        let files = files_of(source, part).await?;
        if format!("{SHA256_PREFIX}{}", content_digest(&files).0) != part_ref.content_hash {
            log::digest_mismatch(&part_ref.id, &part_ref.repository);
            return Err(ResolveError::DigestMismatch {
                part: part_ref.id.clone(),
            });
        }
        let part_dir = staging_dir.join(ARTEFACTS_DIR).join(&hex);
        fs::create_dir_all(&part_dir).map_err(|e| store_failed(&e))?;
        for (name, bytes) in &files.0 {
            if path_unsafe(name) {
                return Err(ResolveError::ManifestInvalid {
                    part: part_ref.id.clone(),
                    code: "AM_PATH_INVALID".to_owned(),
                });
            }
            write_synced(&part_dir.join(name), bytes)?;
            bytes_total = bytes_total.saturating_add(bytes.len() as u64);
        }
        written.insert(hex);
    }
    // Step 18: the lock file.
    let lock_file = lock_of(plan);
    let lock_file_bytes = lock_file.to_canonical_yaml().into_bytes();
    write_synced(&staging_dir.join(LOCK_FILE_NAME), &lock_file_bytes)?;
    // Step 19: the bundle manifest, without a time field.
    let manifest = BundleManifest {
        bundle_schema: BUNDLE_SCHEMA.to_owned(),
        plan_hash: plan.plan_hash.clone(),
        composition: plan.root.clone(),
        artefacts: plan
            .resolved
            .iter()
            .map(|part| BundleArtefact {
                part: part.clone(),
                loading: loading_of(part),
            })
            .collect(),
        overlays_applied: plan.overlay_order.clone(),
        resolved_tool_policy: policy_of(plan),
        lockfile_sha256: sha256_hex(&lock_file_bytes),
        compiler_version: compiler_version.to_owned(),
    };
    let manifest_value =
        serde_json::to_value(&manifest).map_err(|_| ResolveError::BundleCorrupt {
            plan_hash: plan.plan_hash.clone(),
        })?;
    write_synced(
        &staging_dir.join(BUNDLE_MANIFEST_NAME),
        &canonical_json(&manifest_value),
    )?;
    // Step 20: publish by rename; a concurrent compile of the same plan may have won.
    if final_dir.exists() && open_bundle(root, &plan.plan_hash).is_err() {
        fs::remove_dir_all(&final_dir).map_err(|e| store_failed(&e))?;
    }
    match fs::rename(&staging_dir, &final_dir) {
        Ok(()) => staging.published = true,
        Err(error) => {
            if !final_dir.join(BUNDLE_MANIFEST_NAME).exists() {
                return Err(store_failed(&error));
            }
        }
    }
    drop(staging);
    log::compiled(
        &plan.plan_hash,
        plan.resolved.len(),
        bytes_total,
        false,
        started.elapsed(),
    );
    Ok(Bundle {
        plan: plan.clone(),
        manifest,
        lock_file,
        lock_file_bytes,
        dir: final_dir,
    })
}

/// Step 21: an existing bundle by plan hash (`sha256:<hex>`), every digest re-checked.
pub(crate) fn open_bundle(root: &Path, plan_hash: &str) -> Result<Bundle, ResolveError> {
    let not_found = || ResolveError::BundleNotFound {
        plan_hash: plan_hash.to_owned(),
    };
    let corrupt = || ResolveError::BundleCorrupt {
        plan_hash: plan_hash.to_owned(),
    };
    let hex = hex_of(plan_hash);
    if !is_hex_digest(hex) {
        return Err(not_found());
    }
    let dir = root.join(hex);
    let manifest_bytes = fs::read(dir.join(BUNDLE_MANIFEST_NAME)).map_err(|_| not_found())?;
    let manifest: BundleManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|_| corrupt())?;
    let lock_file_bytes = fs::read(dir.join(LOCK_FILE_NAME)).map_err(|_| corrupt())?;
    if manifest.plan_hash != format!("{SHA256_PREFIX}{hex}")
        || sha256_hex(&lock_file_bytes) != manifest.lockfile_sha256
    {
        return Err(corrupt());
    }
    let lock_file = LockFile::from_yaml(&lock_file_bytes).map_err(|_| corrupt())?;
    let bundle_ref = Bundle {
        plan: ResolutionPlan {
            plan_hash: manifest.plan_hash.clone(),
            root: manifest.composition.clone(),
            target_harness: lock_file.target_harness.clone(),
            overlay_order: manifest.overlays_applied.clone(),
            practice_packs: Vec::new(),
            resolved: lock_file.artefacts.clone(),
            selections: lock_file.selections.clone(),
            warnings: Vec::new(),
            // Neither the manifest nor the lock file holds the withdrawn snapshot, so a plan read
            // back from disk carries an empty one.
            withdrawn_snapshot: String::new(),
            tool_policy: Some(manifest.resolved_tool_policy.clone()),
        },
        manifest: manifest.clone(),
        lock_file: lock_file.clone(),
        lock_file_bytes: lock_file_bytes.clone(),
        dir,
    };
    let mut practice_packs = Vec::new();
    for selected in &lock_file.selections.practice_packs {
        let pack = lock_file
            .artefacts
            .iter()
            .find(|a| a.kind == ordin_agent_model::PartKind::PracticePack && a.id == selected.id)
            .ok_or_else(corrupt)?;
        practice_packs.push(pack.clone());
    }
    for part in &lock_file.artefacts {
        bundle_ref.read_part_files(part).map_err(|_| corrupt())?;
    }
    let mut bundle = bundle_ref;
    bundle.plan.practice_packs = practice_packs;
    Ok(bundle)
}

/// A directory's size in bytes, counting what it holds.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                entry.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Step 25: remove every bundle directory whose plan hash is not in `keep`, and every stale staging
/// directory. An I/O error on one directory is logged and the next is tried.
pub(crate) fn prune_root(
    root: &Path,
    keep: &BTreeSet<String>,
) -> Result<PruneReport, ResolveError> {
    let mut report = PruneReport::default();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log::pruned(0, 0, 0);
            return Ok(report);
        }
        Err(error) => return Err(store_failed(&error)),
    };
    let keep: BTreeSet<&str> = keep.iter().map(|hash| hex_of(hash)).collect();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let doomed = if is_hex_digest(&name) {
            !keep.contains(name.as_str())
        } else if name.starts_with(STAGING_PREFIX) {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > STALE_STAGING)
        } else {
            continue;
        };
        if !doomed {
            if is_hex_digest(&name) {
                report.kept += 1;
            }
            continue;
        }
        let size = dir_size(&path);
        match fs::remove_dir_all(&path) {
            Ok(()) => {
                report.removed += 1;
                report.bytes_freed = report.bytes_freed.saturating_add(size);
            }
            Err(error) => {
                tracing::warn!(
                    error_code = ResolveError::StoreWriteFailed { retryable: true }.code(),
                    detail = error.kind().to_string().as_str(),
                    "resolver.prune_failed"
                );
            }
        }
    }
    log::pruned(report.removed, report.kept, report.bytes_freed);
    Ok(report)
}
