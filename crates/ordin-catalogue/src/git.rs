//! Git in process (`git2`, SA-GIT-01): fetch of one branch, tree reads, blob reads, worktrees.
//! Nothing here runs a `git` program, and nothing logs a token or an address beyond host and
//! `owner/name`.

use std::collections::BTreeMap;
use std::path::Path;

use git2::{
    build::CheckoutBuilder, BranchType, ErrorClass, ErrorCode, FetchOptions, ObjectType, Oid,
    RemoteCallbacks, Repository, WorktreeAddOptions, WorktreePruneOptions,
};
use ordin_format_plugins::{MAX_FILE_BYTES, MAX_TREE_BYTES, MAX_TREE_FILES};

/// Why a git operation failed, in the three ways the fetch loop distinguishes (CatalogueService.md §C2).
#[derive(Debug)]
pub(crate) enum GitFailure {
    /// The credentials callback was refused by the host or the sign-in is missing.
    SignIn,
    /// The network or the host did not answer.
    Unreachable(String),
    /// Any other git error.
    Other(String),
}

/// A reply of the credentials callback: user name and token, or why there are none
/// ([`GitFailure::SignIn`] or [`GitFailure::Unreachable`]).
pub(crate) type CredentialFn<'a> = dyn Fn() -> Result<(String, String), GitFailure> + 'a;

pub(crate) fn remote_url(host: &str, repository: &str) -> String {
    format!("https://{host}/{repository}.git")
}

fn classify(error: &git2::Error) -> GitFailure {
    if error.code() == ErrorCode::Auth {
        return GitFailure::SignIn;
    }
    match error.class() {
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssl | ErrorClass::Os => {
            GitFailure::Unreachable(error.message().to_owned())
        }
        _ => GitFailure::Other(error.message().to_owned()),
    }
}

/// Fetches `refs/heads/<branch>` of `url` into `refs/remotes/origin/<branch>` of the repository at
/// `path`, creating the repository when the folder holds none, and returns the fetched head.
pub(crate) fn fetch_branch(
    path: &Path,
    url: &str,
    branch: &str,
    credentials: &CredentialFn<'_>,
) -> Result<String, GitFailure> {
    let other = |e: git2::Error| GitFailure::Other(e.message().to_owned());
    // A GitHub repository goes over SSH when the person chose a key (ProjectService.md PROJ-SSH-1;
    // amended January 2026; user decision; Approved by the user January 2026).
    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
    let route = ordin_projects::ssh::github_route(url);
    let url = route.as_ref().map_or(url, |(ssh, _)| ssh.as_str());
    let repo = match Repository::open(path) {
        Ok(repo) => repo,
        Err(_) => {
            std::fs::create_dir_all(path).map_err(|e| GitFailure::Other(e.to_string()))?;
            restrict_folder(path);
            Repository::init(path).map_err(other)?
        }
    };
    let mut remote = match repo.find_remote("origin") {
        Ok(remote) => {
            if remote.url().ok() != Some(url) {
                repo.remote_set_url("origin", url).map_err(other)?;
            }
            repo.find_remote("origin").map_err(other)?
        }
        Err(_) => repo.remote("origin", url).map_err(other)?,
    };
    let refused: std::cell::RefCell<Option<GitFailure>> = std::cell::RefCell::new(None);
    let mut callbacks = RemoteCallbacks::new();
    match &route {
        Some((_, key)) => ordin_projects::ssh::add_callbacks(&mut callbacks, key),
        None => {
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
    let fetched = remote.fetch(&[refspec.as_str()], Some(&mut options), None);
    if let Err(error) = fetched {
        return Err(refused.take().unwrap_or_else(|| classify(&error)));
    }
    let head = repo
        .refname_to_id(&format!("refs/remotes/origin/{branch}"))
        .map_err(|e| GitFailure::Other(e.message().to_owned()))?;
    Ok(head.to_string())
}

#[cfg(unix)]
fn restrict_folder(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_folder(_path: &Path) {}

/// Creates `path` and its missing parents, each new folder with mode `0700`.
pub(crate) fn create_private_folder(path: &Path) -> std::io::Result<()> {
    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(folder) = cursor {
        if folder.exists() {
            break;
        }
        missing.push(folder.to_path_buf());
        cursor = folder.parent();
    }
    for folder in missing.into_iter().rev() {
        match std::fs::create_dir(&folder) {
            Ok(()) => {}
            // Another task made it between the check above and here (the start-up fetch creates
            // the catalogue root while a probe does): the folder is there, which is all that is
            // asked, so it is restricted like a new one.
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && folder.is_dir() => {}
            Err(error) => return Err(error),
        }
        restrict_folder(&folder);
    }
    Ok(())
}

/// True when the folder opens as a repository and its `HEAD` peels to a commit.
pub(crate) fn clone_is_intact(path: &Path) -> bool {
    Repository::open(path)
        .and_then(|repo| {
            repo.head()
                .and_then(|head| head.peel_to_commit())
                .map(|_| ())
        })
        .is_ok()
}

/// Points the local branch and `HEAD` at `commit` and checks its tree out into the working tree
/// (§D1: `checkout_tree` with force).
pub(crate) fn checkout_commit(path: &Path, branch: &str, commit: &str) -> Result<(), String> {
    let text = |e: git2::Error| e.message().to_owned();
    let repo = Repository::open(path).map_err(text)?;
    let oid = Oid::from_str(commit).map_err(text)?;
    let object = repo
        .find_object(oid, Some(ObjectType::Commit))
        .map_err(text)?;
    let reference = format!("refs/heads/{branch}");
    repo.reference(&reference, oid, true, "catalogue: followed commit")
        .map_err(text)?;
    repo.set_head(&reference).map_err(text)?;
    let mut checkout = CheckoutBuilder::new();
    checkout.force();
    repo.checkout_tree(&object, Some(&mut checkout))
        .map_err(text)?;
    Ok(())
}

/// Why a tree could not be read into memory.
#[derive(Debug)]
pub(crate) enum TreeFailure {
    TooLarge,
    Other(String),
}

/// Every blob of the commit's tree, keyed by its '/'-separated repository path.
pub(crate) fn read_commit_tree(
    path: &Path,
    commit: &str,
) -> Result<BTreeMap<String, Vec<u8>>, TreeFailure> {
    let text = |e: git2::Error| TreeFailure::Other(e.message().to_owned());
    let repo = Repository::open(path).map_err(text)?;
    let commit = repo
        .find_commit(Oid::from_str(commit).map_err(text)?)
        .map_err(text)?;
    let tree = commit.tree().map_err(text)?;
    let mut files = BTreeMap::new();
    let mut total: u64 = 0;
    let mut too_large = false;
    let mut failure: Option<String> = None;
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() != Some(ObjectType::Blob) || entry.filemode() == 0o120000 {
            return git2::TreeWalkResult::Ok;
        }
        let Ok(name) = entry.name() else {
            return git2::TreeWalkResult::Ok;
        };
        let blob = match repo.find_blob(entry.id()) {
            Ok(blob) => blob,
            Err(e) => {
                failure = Some(e.message().to_owned());
                return git2::TreeWalkResult::Abort;
            }
        };
        total += blob.size() as u64;
        if files.len() >= MAX_TREE_FILES || total > MAX_TREE_BYTES || blob.size() > MAX_FILE_BYTES {
            too_large = true;
            return git2::TreeWalkResult::Abort;
        }
        files.insert(format!("{dir}{name}"), blob.content().to_vec());
        git2::TreeWalkResult::Ok
    })
    .or_else(|e| {
        if too_large || failure.is_some() {
            Ok(())
        } else {
            Err(e)
        }
    })
    .map_err(text)?;
    if too_large {
        return Err(TreeFailure::TooLarge);
    }
    if let Some(message) = failure {
        return Err(TreeFailure::Other(message));
    }
    Ok(files)
}

/// The bytes of one file at one commit; `None` when the commit or the file is not in the clone.
pub(crate) fn read_blob_at(
    path: &Path,
    commit: &str,
    file: &str,
) -> Result<Option<Vec<u8>>, String> {
    let text = |e: git2::Error| e.message().to_owned();
    let repo = Repository::open(path).map_err(text)?;
    let Ok(oid) = Oid::from_str(commit) else {
        return Ok(None);
    };
    let Ok(commit) = repo.find_commit(oid) else {
        return Ok(None);
    };
    let tree = commit.tree().map_err(text)?;
    let Ok(entry) = tree.get_path(Path::new(file)) else {
        return Ok(None);
    };
    let found = repo
        .find_blob(entry.id())
        .map(|blob| blob.content().to_vec())
        .ok();
    Ok(found)
}

/// The commit time of a commit in milliseconds since the epoch; `None` when it is not in the clone.
pub(crate) fn commit_time_millis(path: &Path, commit: &str) -> Option<i64> {
    let repo = Repository::open(path).ok()?;
    let commit = repo.find_commit(Oid::from_str(commit).ok()?).ok()?;
    Some(commit.time().seconds() * 1000)
}

/// The reason a worktree could not be opened.
#[derive(Debug)]
pub(crate) enum WorktreeFailure {
    BranchExists,
    Other(String),
}

/// A worktree of the clone on a new branch from the protected branch head (§M).
pub(crate) fn add_worktree(
    clone: &Path,
    protected_branch: &str,
    branch: &str,
    name: &str,
    at: &Path,
) -> Result<String, WorktreeFailure> {
    let text = |e: git2::Error| WorktreeFailure::Other(e.message().to_owned());
    let repo = Repository::open(clone).map_err(text)?;
    if repo.find_branch(branch, BranchType::Local).is_ok() {
        return Err(WorktreeFailure::BranchExists);
    }
    let head = repo
        .refname_to_id(&format!("refs/remotes/origin/{protected_branch}"))
        .map_err(text)?;
    let commit = repo.find_commit(head).map_err(text)?;
    let created = repo.branch(branch, &commit, false).map_err(text)?;
    if let Some(parent) = at.parent() {
        create_private_folder(parent).map_err(|e| WorktreeFailure::Other(e.to_string()))?;
    }
    let mut options = WorktreeAddOptions::new();
    options.reference(Some(created.get()));
    repo.worktree(name, at, Some(&options)).map_err(text)?;
    Ok(head.to_string())
}

/// Prunes the worktree of a draft branch and removes its folder.
pub(crate) fn remove_worktree(clone: &Path, name: &str, at: &Path) -> Result<(), String> {
    let text = |e: git2::Error| e.message().to_owned();
    let repo = Repository::open(clone).map_err(text)?;
    if let Ok(worktree) = repo.find_worktree(name) {
        let mut options = WorktreePruneOptions::new();
        options.valid(true).working_tree(true).locked(true);
        worktree.prune(Some(&mut options)).map_err(text)?;
    }
    if at.exists() {
        std::fs::remove_dir_all(at).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// True for a valid branch name of at most 255 bytes.
pub(crate) fn valid_branch(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && git2::Reference::is_valid_name(&format!("refs/heads/{name}"))
}

/// Every file under a folder of the working tree or a worktree, keyed by '/'-separated path.
pub(crate) fn read_folder(root: &Path) -> std::io::Result<BTreeMap<String, Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if entry.file_name() == ".git" {
                    continue;
                }
                walk(root, &path, out)?;
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                let key = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(key, std::fs::read(&path)?);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod private_folder_tests {
    use super::create_private_folder;

    // Two tasks creating the same folders at once (the start-up fetch and a probe both make the
    // catalogue root) both succeed; before, the slower one failed with "File exists".
    #[test]
    fn folders_made_at_the_same_time_by_two_tasks_are_not_an_error() {
        for _ in 0..50 {
            let root = std::env::temp_dir().join(format!("cat-pf-{}", uuid::Uuid::now_v7()));
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
            let workers: Vec<_> = (0..4)
                .map(|n| {
                    let barrier = barrier.clone();
                    let path = root.join("catalogue").join(format!("task-{n}"));
                    std::thread::spawn(move || {
                        barrier.wait();
                        create_private_folder(&path)
                    })
                })
                .collect();
            for worker in workers {
                worker.join().expect("the thread").expect("the folders");
            }
            assert!(root.join("catalogue").is_dir());
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    // A file in the way is still an error.
    #[test]
    fn a_file_where_a_folder_belongs_is_an_error() {
        let root = std::env::temp_dir().join(format!("cat-pf-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("file"), b"x").unwrap();
        assert!(create_private_folder(&root.join("file").join("below")).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
