//! Every `git2` operation of the service, each a plain blocking function on a repository path. The
//! callers run them on a blocking thread through [`blocking`]. Git errors are logged by class only:
//! a libgit2 message can quote a URL or a path.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use git2::{
    build::CheckoutBuilder, AutotagOption, BranchType, Cred, Delta, Diff, DiffOptions, Direction,
    ErrorClass, ErrorCode, FetchOptions, IndexAddOption, MergeFileInput, MergeFileOptions, Oid,
    Patch, PushOptions, Remote, RemoteCallbacks, Repository, Sort, StatusOptions,
    WorktreeAddOptions, WorktreePruneOptions,
};
use ordin_types::time::Timestamp;

use crate::error::ProjectError;
use crate::types::{CommitSummary, StatusEntry};

/// Runs one blocking step on a blocking thread.
pub(crate) async fn blocking<T, F>(step: F) -> Result<T, ProjectError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ProjectError> + Send + 'static,
{
    tokio::task::spawn_blocking(step)
        .await
        .map_err(|_| ProjectError::GitFailed)?
}

/// Runs a network step: past `limit` the step is told to stop and the caller gets
/// `PROJ_REMOTE_UNREACHABLE`.
pub(crate) async fn network<T, F>(limit: Duration, step: F) -> Result<T, ProjectError>
where
    T: Send + 'static,
    F: FnOnce(Arc<AtomicBool>) -> Result<T, ProjectError> + Send + 'static,
{
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let task = tokio::task::spawn_blocking(move || step(flag));
    match tokio::time::timeout(limit, task).await {
        Ok(joined) => joined.map_err(|_| ProjectError::GitFailed)?,
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            Err(ProjectError::RemoteUnreachable)
        }
    }
}

fn failed(error: git2::Error) -> ProjectError {
    crate::log::git_failed(class_name(&error));
    ProjectError::GitFailed
}

fn class_name(error: &git2::Error) -> &'static str {
    match error.class() {
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssl | ErrorClass::Ssh => "network",
        ErrorClass::Reference => "reference",
        ErrorClass::Repository | ErrorClass::Worktree => "repository",
        ErrorClass::Index | ErrorClass::Tree | ErrorClass::Object | ErrorClass::Odb => "object",
        ErrorClass::Os | ErrorClass::Filesystem => "file_system",
        _ => "other",
    }
}

/// A failure of a network operation (Internal Logic 3.2, 3.3). A token the host refuses is
/// `PROJ_REMOTE_UNREACHABLE` (SA-CLOSE-01). libgit2 reports the HTTP status in its message and
/// no header; a 429, or a 403 naming a rate limit, reads as the host's rate limit.
fn network_failed(error: git2::Error) -> ProjectError {
    let text = error.message().to_ascii_lowercase();
    if text.contains("429") || (text.contains("403") && text.contains("rate limit")) {
        return ProjectError::RateLimited;
    }
    let network = matches!(
        error.class(),
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssl | ErrorClass::Os
    ) || error.code() == ErrorCode::Auth;
    crate::log::git_failed(class_name(&error));
    if network {
        ProjectError::RemoteUnreachable
    } else {
        ProjectError::GitFailed
    }
}

/// `git2::Repository::open`; a folder with no repository is `PROJ_NOT_A_REPOSITORY`.
pub(crate) fn open(path: &Path) -> Result<Repository, ProjectError> {
    Repository::open(path).map_err(|error| match error.code() {
        ErrorCode::NotFound => ProjectError::NotARepository,
        _ => failed(error),
    })
}

/// The branch `HEAD` names, the unborn branch of an empty repository, or the short commit id of a
/// detached `HEAD` (Internal Logic 2.1).
pub(crate) fn current_branch(repo: &Repository) -> Result<String, ProjectError> {
    let head = repo.find_reference("HEAD").map_err(failed)?;
    if let Some(target) = head.symbolic_target().map_err(failed)? {
        return Ok(target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_owned());
    }
    let commit = head.peel_to_commit().map_err(failed)?;
    let short = commit.as_object().short_id().map_err(failed)?;
    Ok(short.as_str().unwrap_or_default().to_owned())
}

/// Whether the folder is a repository, and its current branch.
pub(crate) fn detect(path: &Path) -> Result<Option<String>, ProjectError> {
    match open(path) {
        Ok(repo) => current_branch(&repo).map(Some),
        Err(ProjectError::NotARepository) => Ok(None),
        Err(other) => Err(other),
    }
}

/// `origin`'s URL as the repository's configuration states it, before any `insteadOf` rewrite. It
/// chooses the sign-in; [`open_origin`] checks where the network operation would really go.
pub(crate) fn origin_url(path: &Path) -> Result<Option<String>, ProjectError> {
    let repo = open(path)?;
    let config = repo.config().map_err(failed)?;
    match config.get_string("remote.origin.url") {
        Ok(url) => Ok(Some(url)),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(failed(error)),
    }
}

/// What identifies a repository whatever folder it is in: the origin address and the root commit
/// reached by first parents from HEAD. Either is None when the repository has none, or the folder
/// is not one.
pub(crate) fn repository_identity(path: &Path) -> (Option<String>, Option<String>) {
    let origin = origin_url(path).ok().flatten();
    let root = open(path).ok().and_then(|repo| root_commit(&repo));
    (origin, root)
}

fn root_commit(repo: &Repository) -> Option<String> {
    let mut walk = repo.revwalk().ok()?;
    walk.push_head().ok()?;
    walk.simplify_first_parent().ok()?;
    walk.filter_map(Result::ok)
        .last()
        .map(|oid| oid.to_string())
}

/// The user name and token the credentials callback answers with, and the one host they may go to.
pub(crate) struct NetAuth {
    pub username: &'static str,
    pub token: String,
    pub host: String,
    /// The SSH key the person chose, by path, when GitHub goes over SSH (PROJ-SSH-1). `None` → HTTPS
    /// with the token.
    pub ssh_key: Option<PathBuf>,
    /// Test only: the `file://` address `insteadOf` carries `origin` to in the git tests. Production
    /// has no such field, so every effective URL there is https on `host`.
    #[cfg(any(test, feature = "test-support"))]
    pub local_remote: Option<String>,
    /// Test only: the stand-in host's address, ending in `/` (PROJ-SEAM-2). Production has no such
    /// field.
    #[cfg(any(test, feature = "test-support"))]
    pub test_address: Option<String>,
}

impl NetAuth {
    /// https on the expected host, or, in a test build with a stand-in host, under its address.
    fn allows(&self, url: &str) -> bool {
        // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
        #[cfg(any(test, feature = "test-support"))]
        if let Some(address) = &self.test_address {
            return url.starts_with(address.as_str());
        }
        crate::remote::https_host(url).as_deref() == Some(self.host.as_str())
    }
}

/// `origin` as the network operation will use it. `find_remote` applies `url.*.insteadOf` and
/// `pushInsteadOf`, and `remote.origin.pushurl` is a second address, so the URL classified from the
/// configuration says nothing about where the token goes. Both effective URLs must pass, before
/// any connection is made (Internal Logic 3.1).
fn open_origin<'r>(repo: &'r Repository, auth: &NetAuth) -> Result<Remote<'r>, ProjectError> {
    let remote = repo
        .find_remote("origin")
        .map_err(|_| ProjectError::RemoteUnreachable)?;
    // PROJ-SSH-1: with a key chosen, a GitHub `origin` is reached at its SSH address, whatever form
    // `origin` is written in. Both effective URLs must name the same GitHub repository.
    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
    if auth.ssh_key.is_some() && auth.host == crate::ssh::GITHUB_HOST {
        let named = |url: Option<&[u8]>| {
            url.and_then(|url| std::str::from_utf8(url).ok())
                .and_then(crate::ssh::github_repository)
        };
        let Some(repository) = named(Some(remote.url_bytes())) else {
            crate::log::unsupported_host();
            return Err(ProjectError::RemoteUnreachable);
        };
        if remote
            .pushurl_bytes()
            .is_some_and(|push| named(Some(push)).as_deref() != Some(repository.as_str()))
        {
            crate::log::unsupported_host();
            return Err(ProjectError::RemoteUnreachable);
        }
        return repo
            .remote_anonymous(&crate::ssh::github_ssh_url(&repository))
            .map_err(|_| ProjectError::RemoteUnreachable);
    }
    let urls = [Some(remote.url_bytes()), remote.pushurl_bytes()];
    for url in urls.into_iter().flatten() {
        let allowed = std::str::from_utf8(url).is_ok_and(|url| {
            #[cfg(any(test, feature = "test-support"))]
            if auth.local_remote.as_deref() == Some(url) {
                return true;
            }
            auth.allows(url)
        });
        if !allowed {
            crate::log::unsupported_host();
            return Err(ProjectError::RemoteUnreachable);
        }
    }
    Ok(remote)
}

/// The credentials callback: the token, only for https on the expected host. The transport names
/// the URL it is about to authenticate to, which a redirect can have changed since [`open_origin`].
/// The refusal's message quotes no URL.
fn credential_for(url: &str, auth: &NetAuth) -> Result<Cred, git2::Error> {
    if auth.allows(url) {
        Cred::userpass_plaintext(auth.username, &auth.token)
    } else {
        Err(git2::Error::new(
            ErrorCode::Auth,
            ErrorClass::Http,
            "the credentials are not for this host",
        ))
    }
}

fn callbacks<'a>(auth: &'a NetAuth, stop: Arc<AtomicBool>) -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-2
    match &auth.ssh_key {
        Some(key) => crate::ssh::add_callbacks(&mut callbacks, key),
        None => {
            callbacks.credentials(move |url, _, _| credential_for(url, auth));
        }
    }
    callbacks.transfer_progress(move |_| !stop.load(Ordering::SeqCst));
    callbacks
}

/// Fetches only `refs/heads/<branch>:refs/remotes/origin/<branch>`: no checkout, no change to the
/// working copy or `HEAD`. Returns the remote tip.
pub(crate) fn fetch_branch(
    path: &Path,
    branch: &str,
    auth: &NetAuth,
    stop: Arc<AtomicBool>,
) -> Result<String, ProjectError> {
    let repo = open(path)?;
    let mut remote = open_origin(&repo, auth)?;
    let mut options = FetchOptions::new();
    options.remote_callbacks(callbacks(auth, stop));
    options.download_tags(AutotagOption::None);
    let refspec = format!("+refs/heads/{branch}:refs/remotes/origin/{branch}");
    remote
        .fetch(&[refspec.as_str()], Some(&mut options), None)
        .map_err(network_failed)?;
    let tip = repo
        .refname_to_id(&format!("refs/remotes/origin/{branch}"))
        .map_err(failed)?;
    Ok(tip.to_string())
}

/// The remote `HEAD` symbolic ref; `None` when the remote names none.
pub(crate) fn remote_default_branch(
    path: &Path,
    auth: &NetAuth,
    stop: Arc<AtomicBool>,
) -> Result<Option<String>, ProjectError> {
    let repo = open(path)?;
    let mut remote = open_origin(&repo, auth)?;
    let connection = remote
        .connect_auth(Direction::Fetch, Some(callbacks(auth, stop)), None)
        .map_err(network_failed)?;
    let name = connection.default_branch().ok().and_then(|buf| {
        buf.as_str()
            .ok()
            .and_then(|name| name.strip_prefix("refs/heads/"))
            .map(str::to_owned)
    });
    Ok(name)
}

/// The blob id of `path` in the tree of `commit`, when it is there.
pub(crate) fn blob_at(
    repo: &Repository,
    commit: Oid,
    path: &str,
) -> Result<Option<Oid>, ProjectError> {
    let tree = repo
        .find_commit(commit)
        .and_then(|c| c.tree())
        .map_err(failed)?;
    match tree.get_path(Path::new(path)) {
        Ok(entry) => Ok(Some(entry.id())),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(None),
        Err(error) => Err(failed(error)),
    }
}

/// What a followed path's new state was caused by.
pub(crate) struct PathChange {
    pub blob: Option<String>,
    pub changed: Option<ChangeCommit>,
}

pub(crate) struct ChangeCommit {
    pub commit: String,
    pub author: String,
    pub committed_at: Timestamp,
}

/// Internal Logic 4.4: the blob at the new tip and, when it differs from `last_blob` and that is
/// known, the newest first-parent commit between the two tips that touched the path.
pub(crate) fn path_change(
    path: &Path,
    file: &str,
    tip: &str,
    previous_tip: Option<&str>,
    last_blob: Option<&str>,
    first_fetch: bool,
) -> Result<PathChange, ProjectError> {
    let repo = open(path)?;
    let tip = Oid::from_str(tip).map_err(failed)?;
    let blob = blob_at(&repo, tip, file)?.map(|id| id.to_string());
    if first_fetch || blob.as_deref() == last_blob {
        return Ok(PathChange {
            blob,
            changed: None,
        });
    }
    let mut walk = repo.revwalk().map_err(failed)?;
    walk.simplify_first_parent().map_err(failed)?;
    walk.push(tip).map_err(failed)?;
    if let Some(previous) = previous_tip.and_then(|p| Oid::from_str(p).ok()) {
        // A forced update can leave the old tip unknown or unrelated.
        let _ = walk.hide(previous);
    }
    for id in walk {
        let id = id.map_err(failed)?;
        let commit = repo.find_commit(id).map_err(failed)?;
        let here = blob_at(&repo, id, file)?;
        let before = match commit.parent_id(0) {
            Ok(parent) => blob_at(&repo, parent, file)?,
            Err(_) => None,
        };
        if here != before {
            return Ok(PathChange {
                blob,
                changed: Some(ChangeCommit {
                    commit: id.to_string(),
                    author: commit.author().name().unwrap_or_default().to_owned(),
                    committed_at: commit_time(&commit)?,
                }),
            });
        }
    }
    Ok(PathChange {
        blob,
        changed: None,
    })
}

fn commit_time(commit: &git2::Commit<'_>) -> Result<Timestamp, ProjectError> {
    Timestamp::from_unix_millis(commit.time().seconds().saturating_mul(1000))
        .map_err(|_| ProjectError::GitFailed)
}

/// The blob at `<commit>:<path>` from the object store, never the working copy.
pub(crate) fn read_at_commit(
    path: &Path,
    commit: &str,
    file: &str,
) -> Result<Vec<u8>, ProjectError> {
    let repo = open(path)?;
    let id = repo.revparse_single(commit).map_err(failed)?;
    let tree = id.peel_to_commit().and_then(|c| c.tree()).map_err(failed)?;
    let entry = tree.get_path(Path::new(file)).map_err(failed)?;
    let blob = repo.find_blob(entry.id()).map_err(failed)?;
    Ok(blob.content().to_vec())
}

/// The file at `commit`; `None` when it has no such file.
pub(crate) fn optional_file_at(
    path: &Path,
    commit: &str,
    file: &str,
) -> Result<Option<Vec<u8>>, ProjectError> {
    let repo = open(path)?;
    let oid = Oid::from_str(commit).map_err(failed)?;
    match blob_at(&repo, oid, file)? {
        Some(id) => Ok(Some(repo.find_blob(id).map_err(failed)?.content().to_vec())),
        None => Ok(None),
    }
}

// ---- coding tasks

/// `(base branch, base commit)`: the engineer's current branch and its `HEAD` commit.
pub(crate) fn base_of(path: &Path) -> Result<(String, String), ProjectError> {
    let repo = open(path)?;
    let branch = current_branch(&repo)?;
    let commit = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(failed)?;
    Ok((branch, commit.id().to_string()))
}

pub(crate) fn branch_exists(path: &Path, name: &str) -> Result<bool, ProjectError> {
    let repo = open(path)?;
    let found = repo.find_branch(name, BranchType::Local).map(|_| ());
    match found {
        Ok(()) => Ok(true),
        Err(error) if error.code() == ErrorCode::NotFound => Ok(false),
        Err(error) => Err(failed(error)),
    }
}

pub(crate) fn valid_branch_name(name: &str) -> bool {
    git2::Branch::name_is_valid(name).unwrap_or(false)
}

/// Creates the branch at `commit`, then the worktree holding it. Any failure is
/// `PROJ_WORKTREE_FAILED` and the branch is deleted again (Internal Logic 5.4).
pub(crate) fn add_worktree(
    path: &Path,
    task: &str,
    branch: &str,
    commit: &str,
    worktree: &Path,
) -> Result<(), ProjectError> {
    let repo = open(path)?;
    let added = (|| -> Result<(), git2::Error> {
        let oid = Oid::from_str(commit)?;
        let target = repo.find_commit(oid)?;
        let reference = repo.branch(branch, &target, false)?.into_reference();
        let mut options = WorktreeAddOptions::new();
        options.reference(Some(&reference));
        repo.worktree(task, worktree, Some(&options))?;
        Ok(())
    })();
    added.map_err(|error| {
        crate::log::git_failed(class_name(&error));
        if let Ok(mut created) = repo.find_branch(branch, BranchType::Local) {
            let _ = created.delete();
        }
        ProjectError::WorktreeFailed
    })
}

/// A read-only worktree at `commit` with a detached `HEAD` (Internal Logic 6.1).
pub(crate) fn add_base_worktree(
    path: &Path,
    name: &str,
    commit: &str,
    worktree: &Path,
) -> Result<(), ProjectError> {
    let repo = open(path)?;
    let added = (|| -> Result<(), git2::Error> {
        let oid = Oid::from_str(commit)?;
        let target = repo.find_commit(oid)?;
        let mut branch = repo.branch(&format!("ordin-base/{name}"), &target, false)?;
        {
            let reference = repo.find_reference(branch.get().name()?)?;
            let mut options = WorktreeAddOptions::new();
            options.reference(Some(&reference));
            repo.worktree(name, worktree, Some(&options))?;
        }
        let linked = Repository::open(worktree)?;
        linked.set_head_detached(oid)?;
        branch.delete()?;
        Ok(())
    })();
    added.map_err(|error| {
        crate::log::git_failed(class_name(&error));
        ProjectError::WorktreeFailed
    })
}

/// Internal Logic 5.8: prunes the worktree with the valid and working-tree flags and deletes the
/// local branch, when there is one.
pub(crate) fn remove_worktree(
    path: &Path,
    name: &str,
    worktree: &Path,
    branch: Option<&str>,
) -> Result<(), ProjectError> {
    let repo = open(path).map_err(|_| ProjectError::WorktreeFailed)?;
    let worktree_failed = |error: git2::Error| {
        crate::log::git_failed(class_name(&error));
        ProjectError::WorktreeFailed
    };
    match repo.find_worktree(name) {
        Ok(found) => {
            let mut options = WorktreePruneOptions::new();
            options.valid(true).working_tree(true).locked(true);
            found.prune(Some(&mut options)).map_err(worktree_failed)?;
        }
        Err(error) if error.code() == ErrorCode::NotFound => {}
        Err(error) => return Err(worktree_failed(error)),
    }
    if worktree.exists() {
        std::fs::remove_dir_all(worktree).map_err(|_| ProjectError::WorktreeFailed)?;
    }
    if let Some(branch) = branch {
        match repo.find_branch(branch, BranchType::Local) {
            Ok(mut found) => found.delete().map_err(worktree_failed)?,
            Err(error) if error.code() == ErrorCode::NotFound => {}
            Err(error) => return Err(worktree_failed(error)),
        }
    }
    Ok(())
}

/// Whether `file` under `root` is a regular file reached through no symbolic link. `root` itself is
/// not walked: a worktree's own location is the service's choice.
pub(crate) fn plain_file_in(root: &Path, file: &str) -> bool {
    if !relative_path_ok(file) {
        return false;
    }
    let mut at = root.to_path_buf();
    let mut parts = Path::new(file).components().peekable();
    while let Some(part) = parts.next() {
        at.push(part);
        let Ok(found) = std::fs::symlink_metadata(&at) else {
            return false;
        };
        let wanted = if parts.peek().is_some() {
            found.is_dir()
        } else {
            found.is_file()
        };
        if !wanted {
            return false;
        }
    }
    true
}

/// One file's entries of a change set, as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileHunk {
    pub file: String,
    pub hunk: String,
}

fn hunk_text(patch: &Patch<'_>, index: usize) -> Result<String, git2::Error> {
    let (hunk, lines) = patch.hunk(index)?;
    let mut text = String::from_utf8_lossy(hunk.header()).into_owned();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    for at in 0..lines {
        let line = patch.line_in_hunk(index, at)?;
        let mark = line.origin();
        if !matches!(mark, ' ' | '+' | '-') {
            continue;
        }
        text.push(mark);
        let content = String::from_utf8_lossy(line.content());
        text.push_str(&content);
        if !content.ends_with('\n') {
            text.push_str("\n\\ No newline at end of file\n");
        }
    }
    Ok(text)
}

/// The diff of `base_commit`'s tree to the worktree's working directory, untracked files included
/// (Internal Logic 5.6): one entry per file per hunk; a binary file is one entry; a file with a hunk
/// over 1 MiB is one entry that can only be taken whole.
pub(crate) fn worktree_changes(
    worktree: &Path,
    base_commit: &str,
) -> Result<Vec<FileHunk>, ProjectError> {
    let repo = open(worktree)?;
    let oid = Oid::from_str(base_commit).map_err(failed)?;
    let tree = repo
        .find_commit(oid)
        .and_then(|c| c.tree())
        .map_err(failed)?;
    let mut options = DiffOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    let diff = repo
        .diff_tree_to_workdir_with_index(Some(&tree), Some(&mut options))
        .map_err(failed)?;
    let mut out = Vec::new();
    for index in 0..diff.deltas().len() {
        out.extend(file_hunks(&diff, index).map_err(failed)?);
    }
    Ok(out)
}

fn file_hunks(diff: &Diff<'_>, index: usize) -> Result<Vec<FileHunk>, git2::Error> {
    let Some(delta) = diff.get_delta(index) else {
        return Ok(Vec::new());
    };
    let file = delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let whole = |hunk: &str| {
        vec![FileHunk {
            file: file.clone(),
            hunk: hunk.to_owned(),
        }]
    };
    if delta.status() == Delta::Unmodified {
        return Ok(Vec::new());
    }
    let Some(patch) = Patch::from_diff(diff, index)? else {
        return Ok(whole(crate::BINARY_HUNK));
    };
    if patch.delta().flags().is_binary() {
        return Ok(whole(crate::BINARY_HUNK));
    }
    let mut hunks = Vec::with_capacity(patch.num_hunks());
    for at in 0..patch.num_hunks() {
        let text = hunk_text(&patch, at)?;
        if text.len() > crate::MAX_HUNK_BYTES {
            return Ok(whole(crate::TOO_LARGE_HUNK));
        }
        hunks.push(FileHunk {
            file: file.clone(),
            hunk: text,
        });
    }
    Ok(hunks)
}

/// The outcome of the three-way merge of one file.
pub(crate) struct Merged {
    pub clean: bool,
    pub content: Vec<u8>,
}

/// `git2::merge_file(ancestor, ours, theirs)`. `favour_theirs` takes the conflicting regions from
/// `theirs` (Internal Logic 11.3).
pub(crate) fn merge(
    ancestor: &[u8],
    ours: &[u8],
    theirs: &[u8],
    favour_theirs: bool,
) -> Result<Merged, ProjectError> {
    let (mut base, mut mine, mut other) = (
        MergeFileInput::new(),
        MergeFileInput::new(),
        MergeFileInput::new(),
    );
    base.content(ancestor);
    mine.content(ours);
    other.content(theirs);
    let mut options = MergeFileOptions::new();
    options.our_label("ours").their_label("theirs");
    if favour_theirs {
        options.favor(git2::FileFavor::Theirs);
    }
    let result = git2::merge_file(&base, &mine, &other, Some(&mut options)).map_err(failed)?;
    Ok(Merged {
        clean: result.is_automergeable(),
        content: result.content().to_vec(),
    })
}

// ---- the engineer's views

fn status_word(status: git2::Status) -> &'static str {
    if status.is_conflicted() {
        "conflicted"
    } else if status.is_index_renamed() || status.is_wt_renamed() {
        "renamed"
    } else if status.is_index_new() || status.is_wt_new() {
        "new"
    } else if status.is_index_deleted() || status.is_wt_deleted() {
        "deleted"
    } else {
        "modified"
    }
}

/// Untracked included, ignored excluded.
pub(crate) fn status(path: &Path) -> Result<Vec<StatusEntry>, ProjectError> {
    let repo = open(path)?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repo.statuses(Some(&mut options)).map_err(failed)?;
    Ok(statuses
        .iter()
        .filter_map(|entry| {
            let word = status_word(entry.status());
            entry.path().ok().map(|path| StatusEntry {
                path: path.to_owned(),
                status: word.to_owned(),
            })
        })
        .collect())
}

/// The unified diff of the working copy against `HEAD`, for one path or all.
pub(crate) fn diff(path: &Path, file: Option<&str>) -> Result<String, ProjectError> {
    let repo = open(path)?;
    let tree = match repo.head() {
        Ok(head) => Some(head.peel_to_tree().map_err(failed)?),
        Err(error) if error.code() == ErrorCode::UnbornBranch => None,
        Err(error) => return Err(failed(error)),
    };
    let mut options = DiffOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    if let Some(file) = file {
        options.pathspec(file);
    }
    let diff = repo
        .diff_tree_to_workdir_with_index(tree.as_ref(), Some(&mut options))
        .map_err(failed)?;
    let mut text = Vec::new();
    diff.print(git2::DiffFormat::Patch, |_, _, line| {
        if matches!(line.origin(), ' ' | '+' | '-') {
            text.push(line.origin() as u8);
        }
        text.extend_from_slice(line.content());
        true
    })
    .map_err(failed)?;
    Ok(String::from_utf8_lossy(&text).into_owned())
}

fn summary(commit: &git2::Commit<'_>) -> Result<CommitSummary, ProjectError> {
    Ok(CommitSummary {
        commit: commit.id().to_string(),
        author: commit.author().name().unwrap_or_default().to_owned(),
        at: commit_time(commit)?,
        subject: commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_owned(),
    })
}

/// A revwalk from `HEAD`, path-filtered, at most `limit` commits.
pub(crate) fn history(
    path: &Path,
    file: Option<&str>,
    limit: u32,
) -> Result<Vec<CommitSummary>, ProjectError> {
    let repo = open(path)?;
    let mut walk = repo.revwalk().map_err(failed)?;
    walk.set_sorting(Sort::TIME).map_err(failed)?;
    match walk.push_head() {
        Ok(()) => {}
        Err(error) if error.code() == ErrorCode::UnbornBranch => return Ok(Vec::new()),
        Err(error) => return Err(failed(error)),
    }
    let limit = limit.min(crate::HISTORY_MAX) as usize;
    let mut out = Vec::new();
    for id in walk {
        if out.len() >= limit {
            break;
        }
        let id = id.map_err(failed)?;
        let commit = repo.find_commit(id).map_err(failed)?;
        if let Some(file) = file {
            let here = blob_at(&repo, id, file)?;
            let before = match commit.parent_id(0) {
                Ok(parent) => blob_at(&repo, parent, file)?,
                Err(_) => None,
            };
            if here == before {
                continue;
            }
        }
        out.push(summary(&commit)?);
    }
    Ok(out)
}

/// Local branches except `ordin/*`.
pub(crate) fn branches(path: &Path) -> Result<Vec<String>, ProjectError> {
    let repo = open(path)?;
    let mut names = Vec::new();
    for branch in repo.branches(Some(BranchType::Local)).map_err(failed)? {
        let (branch, _) = branch.map_err(failed)?;
        if let Some(name) = branch.name().map_err(failed)? {
            if !name.starts_with("ordin/") {
                names.push(name.to_owned());
            }
        }
    }
    names.sort();
    Ok(names)
}

pub(crate) fn create_branch(path: &Path, name: &str) -> Result<(), ProjectError> {
    let repo = open(path)?;
    let commit = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(failed)?;
    repo.branch(name, &commit, false)
        .map(|_| ())
        .map_err(|error| match error.code() {
            ErrorCode::Exists => ProjectError::BranchExists,
            _ => failed(error),
        })
}

/// A safe checkout: uncommitted changes the switch would overwrite refuse it, and nothing moves.
pub(crate) fn switch_branch(path: &Path, name: &str) -> Result<(), ProjectError> {
    let repo = open(path)?;
    let reference = repo
        .find_branch(name, BranchType::Local)
        .map_err(failed)?
        .into_reference();
    let tree = reference.peel_to_tree().map_err(failed)?;
    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_tree(tree.as_object(), Some(&mut checkout))
        .map_err(|error| match error.code() {
            ErrorCode::Conflict | ErrorCode::Uncommitted | ErrorCode::Modified => {
                ProjectError::WorkingCopyDirty
            }
            _ => failed(error),
        })?;
    repo.set_head(&format!("refs/heads/{name}")).map_err(failed)
}

/// Stages every change in the working copy and commits it with the repository's `user.name` and
/// `user.email` (Internal Logic 10.2).
pub(crate) fn commit_all(path: &Path, message: &str) -> Result<CommitSummary, ProjectError> {
    let repo = open(path)?;
    let mut index = repo.index().map_err(failed)?;
    index
        .add_all(["*"], IndexAddOption::DEFAULT, None)
        .map_err(failed)?;
    index.update_all(["*"], None).map_err(failed)?;
    index.write().map_err(failed)?;
    let tree_id = index.write_tree().map_err(failed)?;
    let parent = match repo.head() {
        Ok(head) => Some(head.peel_to_commit().map_err(failed)?),
        Err(error) if error.code() == ErrorCode::UnbornBranch => None,
        Err(error) => return Err(failed(error)),
    };
    let unchanged = match &parent {
        Some(parent) => parent.tree_id() == tree_id,
        None => repo.find_tree(tree_id).map_err(failed)?.is_empty(),
    };
    if unchanged {
        return Err(ProjectError::NothingToCommit);
    }
    let tree = repo.find_tree(tree_id).map_err(failed)?;
    let signature = repo.signature().map_err(failed)?;
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    let id = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
        .map_err(failed)?;
    let created = repo.find_commit(id).map_err(failed)?;
    summary(&created)
}

/// Pushes the current branch without force; a rejection is `PROJ_PUSH_REFUSED` and the commit stays
/// local. Returns the commit and the branch.
pub(crate) fn push(
    path: &Path,
    branch: &str,
    auth: &NetAuth,
    stop: Arc<AtomicBool>,
) -> Result<String, ProjectError> {
    let repo = open(path)?;
    let commit = repo
        .head()
        .and_then(|head| head.peel_to_commit())
        .map_err(failed)?
        .id()
        .to_string();
    let mut remote = open_origin(&repo, auth)?;
    let refused = Arc::new(Mutex::new(false));
    let seen = refused.clone();
    let mut calls = callbacks(auth, stop);
    calls.push_update_reference(move |_, status| {
        if status.is_some() {
            if let Ok(mut flag) = seen.lock() {
                *flag = true;
            }
        }
        Ok(())
    });
    let mut options = PushOptions::new();
    options.remote_callbacks(calls);
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    remote
        .push(&[refspec.as_str()], Some(&mut options))
        .map_err(|error| match error.code() {
            ErrorCode::NotFastForward | ErrorCode::Locked | ErrorCode::Exists => {
                ProjectError::PushRefused
            }
            _ if error.message().contains("403") => ProjectError::PushRefused,
            _ => network_failed(error),
        })?;
    if refused.lock().map(|flag| *flag).unwrap_or(true) {
        return Err(ProjectError::PushRefused);
    }
    Ok(commit)
}

/// The folder of a worktree of this project under `root`, relative parts refused.
pub(crate) fn under_root(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
        && !path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
}

/// A relative path with no part leaving its root.
pub(crate) fn relative_path_ok(path: &str) -> bool {
    let path = PathBuf::from(path);
    path.is_relative()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rate_limit_is_read_from_the_transport_message() {
        let limited = git2::Error::from_str("unexpected http status code: 429");
        assert_eq!(network_failed(limited).code(), "PROJ_RATE_LIMITED");
        let forbidden = git2::Error::from_str("403 API rate limit exceeded");
        assert_eq!(network_failed(forbidden).code(), "PROJ_RATE_LIMITED");
        let refused = git2::Error::new(ErrorCode::Auth, ErrorClass::Http, "too many redirects");
        assert_eq!(network_failed(refused).code(), "PROJ_REMOTE_UNREACHABLE");
        let local = git2::Error::new(ErrorCode::GenericError, ErrorClass::Config, "bad");
        assert_eq!(network_failed(local).code(), "PROJ_GIT_FAILED");
    }

    fn auth(host: &str) -> NetAuth {
        NetAuth {
            username: "x-access-token",
            token: "the-token".to_owned(),
            host: host.to_owned(),
            ssh_key: None,
            local_remote: None,
            test_address: None,
        }
    }

    // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
    #[test]
    fn a_test_host_answers_only_under_its_address() {
        let auth = NetAuth {
            test_address: Some("http://127.0.0.1:4123/".to_owned()),
            ..auth("http://127.0.0.1:4123/")
        };
        assert!(credential_for("http://127.0.0.1:4123/remote.git", &auth).is_ok());
        for other in [
            "http://127.0.0.1:41234/remote.git",
            "https://github.com/acme/portal.git",
            "http://127.0.0.1:4123",
        ] {
            assert!(credential_for(other, &auth).is_err(), "{other}");
        }
    }

    // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
    #[test]
    fn a_test_host_is_https_or_loopback_http() {
        use crate::service::test_host_address;
        assert_eq!(
            test_host_address("http://127.0.0.1:4123").as_deref(),
            Some("http://127.0.0.1:4123/")
        );
        assert_eq!(
            test_host_address("https://git.example/").as_deref(),
            Some("https://git.example/")
        );
        for bad in [
            "http://git.example",
            "http://192.168.0.2:80",
            "file:///tmp/remote.git",
            "https://git.example/?x=1",
        ] {
            assert!(test_host_address(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_credentials_callback_answers_only_for_https_on_the_expected_host() {
        let auth = auth("github.com");
        let answered = credential_for("https://github.com/acme/portal.git", &auth).unwrap();
        assert!(answered.has_username());
        // `credtype()` is i32 on Windows and u32 elsewhere, so compare as i64.
        assert_eq!(
            i64::from(answered.credtype()),
            i64::from(git2::CredentialType::USER_PASS_PLAINTEXT.bits())
        );
        for url in [
            "https://evil.example/acme/portal.git",
            "https://github.com.evil.example/acme/portal.git",
            "https://evil.example#@github.com/x",
            "http://github.com/acme/portal.git",
            "ssh://git@github.com/acme/portal.git",
            "file:///tmp/portal.git",
        ] {
            let refused = credential_for(url, &auth).err().expect(url);
            assert_eq!(refused.code(), ErrorCode::Auth, "{url}");
            assert!(!refused.message().contains("evil"), "no URL in the message");
            assert_eq!(network_failed(refused).code(), "PROJ_REMOTE_UNREACHABLE");
        }
    }

    /// `origin` at `https://github.com/acme/portal.git` with one more configuration value.
    fn origin_with(dir: &Path, key: &str, value: &str) -> Result<(), ProjectError> {
        let repo = crate::tests::support::repository(dir, &[("a.txt", "a\n")]);
        repo.remote("origin", "https://github.com/acme/portal.git")
            .unwrap();
        repo.config().unwrap().set_str(key, value).unwrap();
        open_origin(&repo, &auth("github.com")).map(|_| ())
    }

    #[test]
    fn open_origin_checks_the_effective_urls_with_no_network() {
        let root = crate::tests::support::TempDir::new();
        let github = "https://github.com/";
        let allowed = [
            ("user.name", "No rewrite"),
            ("remote.origin.pushurl", "https://GitHub.com/acme/other.git"),
            ("url.https://github.com/mirror/.insteadOf", github),
        ];
        for (at, (key, value)) in allowed.into_iter().enumerate() {
            let dir = root.0.join(format!("allowed-{at}"));
            assert!(origin_with(&dir, key, value).is_ok(), "{key}");
        }
        let refused = [
            ("url.https://evil.example/.insteadOf", github),
            ("url.https://evil.example/.pushInsteadOf", github),
            ("url.http://github.com/.insteadOf", github),
            ("url.https://evil.example#@github.com/.insteadOf", github),
            ("url.https://github.com.evil.example/.insteadOf", github),
            ("remote.origin.pushurl", "https://evil.example/portal.git"),
            ("remote.origin.pushurl", "git@github.com:acme/portal.git"),
        ];
        for (at, (key, value)) in refused.into_iter().enumerate() {
            let dir = root.0.join(format!("refused-{at}"));
            let error = origin_with(&dir, key, value).expect_err(key);
            assert_eq!(error.code(), "PROJ_REMOTE_UNREACHABLE", "{key}");
        }
    }

    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
    #[test]
    fn with_a_key_chosen_a_github_origin_is_reached_at_its_ssh_address() {
        let root = crate::tests::support::TempDir::new();
        let with_key = NetAuth {
            ssh_key: Some(PathBuf::from("/home/sam/.ssh/id_ed25519")),
            ..auth("github.com")
        };
        let cases = [
            ("https://github.com/acme/portal.git", None, true),
            ("git@github.com:acme/portal.git", None, true),
            ("ssh://git@github.com/acme/portal", None, true),
            (
                "https://github.com/acme/portal.git",
                Some("git@github.com:acme/portal.git"),
                true,
            ),
            (
                "https://github.com/acme/portal.git",
                Some("git@github.com:acme/other.git"),
                false,
            ),
            ("https://evil.example/acme/portal.git", None, false),
            ("git@evil.example:acme/portal.git", None, false),
        ];
        for (at, (url, push, reached)) in cases.into_iter().enumerate() {
            let dir = root.0.join(format!("ssh-{at}"));
            let repo = crate::tests::support::repository(&dir, &[("a.txt", "a\n")]);
            repo.remote("origin", url).unwrap();
            if let Some(push) = push {
                repo.config()
                    .unwrap()
                    .set_str("remote.origin.pushurl", push)
                    .unwrap();
            }
            match open_origin(&repo, &with_key) {
                Ok(remote) => {
                    assert!(reached, "{url} {push:?}");
                    assert_eq!(remote.url().ok(), Some("git@github.com:acme/portal.git"));
                }
                Err(error) => {
                    assert!(!reached, "{url} {push:?}");
                    assert_eq!(error.code(), "PROJ_REMOTE_UNREACHABLE");
                }
            };
        }
    }

    #[test]
    fn relative_paths_stay_inside() {
        assert!(relative_path_ok("src/a.rs"));
        assert!(!relative_path_ok("../a.rs"));
        assert!(!relative_path_ok("/etc/passwd"));
        assert!(!relative_path_ok("a/../../b"));
    }
}
