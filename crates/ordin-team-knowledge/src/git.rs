//! The bare clone of a bound repository (§B step 4): fetch, the folder's tree at a tip, blobs, the
//! commit built straight from tree objects (§E step 6), and the history walk of TW-J. Every function
//! is blocking; callers run them on `spawn_blocking`. Nothing is ever written to a working tree.

use std::collections::BTreeMap;
use std::path::Path;

use git2::{ErrorClass, ErrorCode, FetchOptions, Oid, RemoteCallbacks, Repository, Signature};

/// Why a Git step failed, in the terms the Error Table needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFailure {
    /// The host refused the token, or no sign-in exists (`TEAM_SIGN_IN_NEEDED`).
    SignIn,
    /// The network or the host could not be reached (`TEAM_REMOTE_UNREACHABLE` after 3 attempts).
    Unreachable,
    /// The branch does not exist (`TEAM_LINK_NOT_SUITABLE`).
    NoBranch,
    /// The local copy cannot be made or read (`TEAM_CLONE_FAILED`).
    Disk,
}

/// The credentials asked for inside the callback: (username, token) or the failure to report.
pub type Credentials<'a> = dyn Fn() -> Result<(String, String), GitFailure> + 'a;

fn classify(error: &git2::Error) -> GitFailure {
    if error.code() == ErrorCode::Auth {
        return GitFailure::SignIn;
    }
    let message = error.message().to_ascii_lowercase();
    if message.contains("couldn't find remote ref") || message.contains("could not find remote ref")
    {
        return GitFailure::NoBranch;
    }
    if message.contains("401") || message.contains("authentication") {
        return GitFailure::SignIn;
    }
    match error.class() {
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssl | ErrorClass::Ssh => {
            GitFailure::Unreachable
        }
        ErrorClass::Os if message.contains("connect") || message.contains("resolve") => {
            GitFailure::Unreachable
        }
        _ => GitFailure::Disk,
    }
}

fn disk(_: git2::Error) -> GitFailure {
    GitFailure::Disk
}

/// `git2::Repository::init_bare` at `path`, or the existing clone.
pub fn open_or_init(path: &Path, url: &str) -> Result<Repository, GitFailure> {
    let repo = match Repository::open_bare(path) {
        Ok(repo) => repo,
        Err(_) => {
            std::fs::create_dir_all(path).map_err(|_| GitFailure::Disk)?;
            Repository::init_bare(path).map_err(disk)?
        }
    };
    match repo.find_remote("origin") {
        Ok(remote) => {
            if remote.url().ok() != Some(url) {
                repo.remote_set_url("origin", url).map_err(disk)?;
            }
        }
        Err(_) => {
            repo.remote("origin", url).map_err(disk)?;
        }
    }
    Ok(repo)
}

/// One fetch of `refs/heads/<branch>` into `refs/remotes/origin/<branch>`; the fetched tip.
pub fn fetch_once(
    path: &Path,
    url: &str,
    branch: &str,
    credentials: &Credentials<'_>,
) -> Result<Oid, GitFailure> {
    // A GitHub repository goes over SSH when the person chose a key (ProjectService.md PROJ-SSH-1;
    // amended January 2026; user decision; Approved by the user January 2026).
    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
    let route = ordin_projects::ssh::github_route(url);
    let url = route.as_ref().map_or(url, |(ssh, _)| ssh.as_str());
    let repo = open_or_init(path, url)?;
    let mut remote = repo.find_remote("origin").map_err(disk)?;
    let refused: std::cell::RefCell<Option<GitFailure>> = std::cell::RefCell::new(None);
    let mut callbacks = RemoteCallbacks::new();
    match &route {
        Some((_, key)) => ordin_projects::ssh::add_callbacks(&mut callbacks, key),
        None => {
            // The token is read through `expose()` inside this callback only, never stored (Security).
            callbacks.credentials(|_url, _user, allowed| {
                if !allowed.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
                    return Err(git2::Error::from_str("no plain credentials accepted"));
                }
                match credentials() {
                    Ok((user, token)) => git2::Cred::userpass_plaintext(&user, &token),
                    Err(failure) => {
                        *refused.borrow_mut() = Some(failure);
                        Err(git2::Error::from_str("no credentials"))
                    }
                }
            });
        }
    }
    let mut options = FetchOptions::new();
    options.remote_callbacks(callbacks);
    let refspec = format!("+refs/heads/{branch}:refs/remotes/origin/{branch}");
    if let Err(error) = remote.fetch(&[refspec.as_str()], Some(&mut options), None) {
        return Err(refused.take().unwrap_or_else(|| classify(&error)));
    }
    repo.refname_to_id(&format!("refs/remotes/origin/{branch}"))
        .map_err(|_| GitFailure::NoBranch)
}

/// Every file below `folder` at `tip`, by path relative to the folder, with its blob id. A folder
/// absent at the tip is empty.
pub fn folder_files(
    repo: &Repository,
    tip: Oid,
    folder: &str,
) -> Result<BTreeMap<String, Oid>, GitFailure> {
    let commit = repo.find_commit(tip).map_err(disk)?;
    let root = commit.tree().map_err(disk)?;
    let tree = if folder.is_empty() {
        root
    } else {
        match root.get_path(Path::new(folder)) {
            Ok(entry) if entry.kind() == Some(git2::ObjectType::Tree) => {
                repo.find_tree(entry.id()).map_err(disk)?
            }
            _ => return Ok(BTreeMap::new()),
        }
    };
    let mut out = BTreeMap::new();
    walk(repo, &tree, "", &mut out)?;
    Ok(out)
}

fn walk(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    prefix: &str,
    out: &mut BTreeMap<String, Oid>,
) -> Result<(), GitFailure> {
    for entry in tree.iter() {
        let Ok(name) = entry.name() else {
            continue;
        };
        let path = if prefix.is_empty() {
            name.to_owned()
        } else {
            format!("{prefix}/{name}")
        };
        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                out.insert(path, entry.id());
            }
            Some(git2::ObjectType::Tree) => {
                let sub = repo.find_tree(entry.id()).map_err(disk)?;
                walk(repo, &sub, &path, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// A blob's bytes when it is at most `max` bytes; `Ok(None)` when larger (never read whole).
pub fn read_blob(repo: &Repository, oid: Oid, max: u64) -> Result<Option<Vec<u8>>, GitFailure> {
    let odb = repo.odb().map_err(disk)?;
    let (size, _) = odb.read_header(oid).map_err(disk)?;
    if size as u64 > max {
        return Ok(None);
    }
    let blob = repo.find_blob(oid).map_err(disk)?;
    Ok(Some(blob.content().to_vec()))
}

/// The id Git gives these bytes as a blob, written to the clone's object store.
pub fn write_blob(repo: &Repository, bytes: &[u8]) -> Result<Oid, GitFailure> {
    repo.blob(bytes).map_err(disk)
}

/// One change of a share: a path relative to the folder and its new blob, `None` to delete.
pub type Change = (String, Option<Oid>);

/// §E step 6: from `parent`'s tree, writes each change under `folder`, builds the trees bottom-up
/// with `TreeBuilder`, and creates a commit whose only parent is `parent`. No reference is moved.
pub fn commit(
    repo: &Repository,
    parent: Oid,
    folder: &str,
    changes: &[Change],
    author: &Signature<'_>,
    message: &str,
) -> Result<Oid, GitFailure> {
    let parent_commit = repo.find_commit(parent).map_err(disk)?;
    let root = parent_commit.tree().map_err(disk)?;
    let mut full: Vec<(Vec<String>, Option<Oid>)> = changes
        .iter()
        .map(|(path, blob)| {
            let mut parts: Vec<String> = folder
                .split('/')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            parts.extend(path.split('/').map(str::to_owned));
            (parts, *blob)
        })
        .collect();
    full.sort();
    let tree = apply(repo, Some(&root), &full)?.ok_or(GitFailure::Disk)?;
    let tree = repo.find_tree(tree).map_err(disk)?;
    repo.commit(None, author, author, message, &tree, &[&parent_commit])
        .map_err(disk)
}

/// Applies `changes` (paths split in segments) to `base`; `None` when the tree ends up empty.
fn apply(
    repo: &Repository,
    base: Option<&git2::Tree<'_>>,
    changes: &[(Vec<String>, Option<Oid>)],
) -> Result<Option<Oid>, GitFailure> {
    let mut builder = repo.treebuilder(base).map_err(disk)?;
    let mut index = 0;
    while index < changes.len() {
        let head = changes[index].0[0].clone();
        let mut end = index;
        while end < changes.len() && changes[end].0[0] == head {
            end += 1;
        }
        let group = &changes[index..end];
        let leaf = group.iter().find(|(parts, _)| parts.len() == 1);
        if let Some((_, blob)) = leaf {
            match blob {
                Some(oid) => {
                    builder.insert(&head, *oid, 0o100644).map_err(disk)?;
                }
                None => {
                    if builder.get(&head).map_err(disk)?.is_some() {
                        builder.remove(&head).map_err(disk)?;
                    }
                }
            }
        } else {
            let sub_changes: Vec<(Vec<String>, Option<Oid>)> = group
                .iter()
                .map(|(parts, blob)| (parts[1..].to_vec(), *blob))
                .collect();
            let existing = builder
                .get(&head)
                .map_err(disk)?
                .filter(|e| e.kind() == Some(git2::ObjectType::Tree))
                .map(|e| e.id());
            let sub_tree = match existing {
                Some(id) => Some(repo.find_tree(id).map_err(disk)?),
                None => None,
            };
            match apply(repo, sub_tree.as_ref(), &sub_changes)? {
                Some(oid) => {
                    builder.insert(&head, oid, 0o040000).map_err(disk)?;
                }
                None => {
                    if builder.get(&head).map_err(disk)?.is_some() {
                        builder.remove(&head).map_err(disk)?;
                    }
                }
            }
        }
        index = end;
    }
    if builder.is_empty() {
        return Ok(None);
    }
    builder.write().map(Some).map_err(disk)
}

/// True when `tip` is `commit` or descends from it.
pub fn contains(repo: &Repository, tip: Oid, commit: Oid) -> bool {
    tip == commit || repo.graph_descendant_of(tip, commit).unwrap_or(false)
}

/// The blob at `path` below `folder` in commit `commit`, if any.
pub fn blob_at(repo: &Repository, commit: Oid, folder: &str, path: &str) -> Option<Oid> {
    let tree = repo.find_commit(commit).ok()?.tree().ok()?;
    let full = if folder.is_empty() {
        path.to_owned()
    } else {
        format!("{folder}/{path}")
    };
    tree.get_path(Path::new(&full))
        .ok()
        .filter(|e| e.kind() == Some(git2::ObjectType::Blob))
        .map(|e| e.id())
}

/// The commits reachable from `tip`, newest first in topological order, at most `limit`, each with
/// its parents and author name (TW-J step 2).
pub fn history(
    repo: &Repository,
    tip: Oid,
    limit: usize,
) -> Result<Vec<(Oid, Vec<Oid>, String)>, GitFailure> {
    let mut walk = repo.revwalk().map_err(disk)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL).map_err(disk)?;
    walk.push(tip).map_err(disk)?;
    let mut out = Vec::new();
    for oid in walk.take(limit) {
        let oid = oid.map_err(disk)?;
        let commit = repo.find_commit(oid).map_err(disk)?;
        let author = commit.author().name().unwrap_or("").to_owned();
        out.push((oid, commit.parent_ids().collect(), author));
    }
    Ok(out)
}
