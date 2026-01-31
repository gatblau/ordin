//! The in-process Git steps (SA-GIT-01): commit, push without force, and fetch with `git2`, never a
//! shell. A token lives only inside the credentials callback and is never stored or logged.

use std::path::Path;
use std::time::Duration;

use git2::{Cred, ErrorClass, ErrorCode, FetchOptions, PushOptions, RemoteCallbacks, Repository};

/// Every network step of this module is bounded (Constraint C4: timeouts on every external call).
pub(crate) const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// How a Git step failed, by what the destination or the transport answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GitFailure {
    /// The remote branch moved: nothing is forced.
    NonFastForward,
    /// The sign-in was refused (HTTP 401).
    Auth,
    /// The remote refused for permission (HTTP 403).
    Forbidden,
    /// The remote could not be reached; nothing was sent.
    Network,
    /// The step ran out of time; it may have arrived.
    Timeout,
    /// Anything else, as the first line of what Git said (no token, no content).
    Other(String),
}

impl GitFailure {
    /// The registry code of the item, copy or submission outcome (C.6, F.3, I.6).
    pub(crate) fn code(&self) -> &'static str {
        match self {
            GitFailure::NonFastForward => "PUSH_ORIGIN_CHANGED",
            GitFailure::Auth => "PUSH_SIGN_IN_NEEDED",
            GitFailure::Forbidden => "PUSH_FORBIDDEN",
            GitFailure::Network => "PUSH_ORIGIN_UNREACHABLE",
            GitFailure::Timeout => "PUSH_OUTCOME_UNKNOWN",
            GitFailure::Other(_) => "PUSH_DESTINATION_REFUSED",
        }
    }
}

/// A step that precedes the push.
#[derive(Debug)]
pub(crate) enum GitStep {
    /// The checked-out branch is not the branch the item belongs to.
    WrongBranch,
    /// `user.name` or `user.email` is not set.
    IdentityMissing,
    Failed(GitFailure),
}

impl From<GitFailure> for GitStep {
    fn from(failure: GitFailure) -> Self {
        GitStep::Failed(failure)
    }
}

pub(crate) struct Creds {
    pub username: String,
    pub token: String,
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

fn classify_message(message: &str) -> Option<GitFailure> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("non-fast-forward")
        || lower.contains("non-fastforwardable")
        || lower.contains("fetch first")
        || lower.contains("not present locally")
    {
        Some(GitFailure::NonFastForward)
    } else if lower.contains("403")
        || lower.contains("forbidden")
        || lower.contains("permission denied")
    {
        Some(GitFailure::Forbidden)
    } else if lower.contains("timed out") || lower.contains("timeout") {
        Some(GitFailure::Timeout)
    } else {
        None
    }
}

pub(crate) fn classify(error: &git2::Error) -> GitFailure {
    if error.code() == ErrorCode::NotFastForward {
        return GitFailure::NonFastForward;
    }
    if error.code() == ErrorCode::Auth {
        return GitFailure::Auth;
    }
    if let Some(known) = classify_message(error.message()) {
        return known;
    }
    match error.class() {
        ErrorClass::Net | ErrorClass::Http | ErrorClass::Ssl | ErrorClass::Os => {
            GitFailure::Network
        }
        _ => GitFailure::Other(first_line(error.message())),
    }
}

/// `remote_name`, or, for a GitHub repository when the person chose an SSH key, the same repository
/// at its SSH address with that key (ProjectService.md PROJ-SSH-1; amended January 2026; user
/// decision; Approved by the user January 2026).
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
fn find_remote<'r>(
    repo: &'r Repository,
    remote_name: &str,
) -> Result<(git2::Remote<'r>, Option<std::path::PathBuf>), GitFailure> {
    let remote = repo
        .find_remote(remote_name)
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    let route = remote
        .url()
        .ok()
        .and_then(ordin_projects::ssh::github_route);
    match route {
        Some((ssh, key)) => {
            let anonymous = repo
                .remote_anonymous(&ssh)
                .map_err(|e| GitFailure::Other(first_line(e.message())))?;
            Ok((anonymous, Some(key)))
        }
        None => Ok((remote, None)),
    }
}

fn callbacks<'a>(
    creds: Option<&'a Creds>,
    key: Option<&'a std::path::Path>,
) -> RemoteCallbacks<'a> {
    let mut callbacks = RemoteCallbacks::new();
    if let Some(key) = key {
        ordin_projects::ssh::add_callbacks(&mut callbacks, key);
        return callbacks;
    }
    let mut attempts = 0u8;
    callbacks.credentials(move |_url, _user, _allowed| {
        attempts += 1;
        match creds {
            Some(c) if attempts == 1 => Cred::userpass_plaintext(&c.username, &c.token),
            _ => Err(git2::Error::new(
                ErrorCode::Auth,
                ErrorClass::Http,
                "no credentials",
            )),
        }
    });
    callbacks
}

/// Pushes `refspec` (never with a leading `+`) to `remote_name` without force.
pub(crate) fn push_refspec(
    repo: &Repository,
    remote_name: &str,
    refspec: &str,
    creds: Option<&Creds>,
) -> Result<(), GitFailure> {
    debug_assert!(!refspec.starts_with('+'));
    let (mut remote, key) = find_remote(repo, remote_name)?;
    let rejected = std::cell::RefCell::new(None::<String>);
    let mut cb = callbacks(creds, key.as_deref());
    cb.push_update_reference(|_name, status| {
        if let Some(message) = status {
            *rejected.borrow_mut() = Some(message.to_owned());
        }
        Ok(())
    });
    let mut options = PushOptions::new();
    options.remote_callbacks(cb);
    remote
        .push(&[refspec], Some(&mut options))
        .map_err(|e| classify(&e))?;
    drop(options);
    if let Some(message) = rejected.into_inner() {
        return Err(classify_message(&message).unwrap_or(GitFailure::Other(first_line(&message))));
    }
    Ok(())
}

/// Fetches one branch of `remote_name` into `refs/remotes/<remote_name>/<branch>`.
pub(crate) fn fetch_branch(
    repo: &Repository,
    remote_name: &str,
    branch: &str,
    creds: Option<&Creds>,
) -> Result<(), GitFailure> {
    let (mut remote, key) = find_remote(repo, remote_name)?;
    let mut options = FetchOptions::new();
    options.remote_callbacks(callbacks(creds, key.as_deref()));
    let spec = format!("+refs/heads/{branch}:refs/remotes/{remote_name}/{branch}");
    remote
        .fetch(&[spec.as_str()], Some(&mut options), None)
        .map_err(|e| classify(&e))
}

/// The tip of `refs/remotes/<remote_name>/<branch>`.
pub(crate) fn remote_tip(repo: &Repository, remote_name: &str, branch: &str) -> Option<git2::Oid> {
    repo.refname_to_id(&format!("refs/remotes/{remote_name}/{branch}"))
        .ok()
}

/// `true` when `tip` is `commit` or has it as an ancestor.
pub(crate) fn contains_commit(repo: &Repository, tip: git2::Oid, commit: git2::Oid) -> bool {
    tip == commit || repo.graph_descendant_of(tip, commit).unwrap_or(false)
}

/// C.4 (Git): verifies the checked-out branch, adds exactly `relative_path`, and commits with the
/// repository's configured identity. Returns the new commit id.
// spec: ordin-desktop/components/PushService#SA-GIT-01
// spec: ordin-desktop/components/PushService#SA-RECON-02
pub(crate) fn commit_paths(
    folder: &Path,
    expected_branch: Option<&str>,
    paths: &[&str],
    message: &str,
) -> Result<git2::Oid, GitStep> {
    let repo = Repository::open(folder).map_err(|e| GitFailure::Other(first_line(e.message())))?;
    if let Some(expected) = expected_branch {
        let head = repo
            .head()
            .map_err(|e| GitFailure::Other(first_line(e.message())))?;
        if head.shorthand().ok() != Some(expected) {
            return Err(GitStep::WrongBranch);
        }
    }
    let config = repo
        .config()
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    let name = config
        .get_string("user.name")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let email = config
        .get_string("user.email")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let (Some(name), Some(email)) = (name, email) else {
        return Err(GitStep::IdentityMissing);
    };
    let signature = git2::Signature::now(&name, &email)
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    let mut index = repo
        .index()
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    for path in paths {
        index
            .add_path(Path::new(path))
            .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    }
    index
        .write()
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    let tree_id = index
        .write_tree()
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    let tree = repo
        .find_tree(tree_id)
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
    let oid = repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
        .map_err(|e| GitFailure::Other(first_line(e.message())))?;
    Ok(oid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use git2::{Repository, Signature};

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            let path =
                std::env::temp_dir().join(format!("ordin-push-git-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).expect("temporary folder");
            Dir(path)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    use std::path::PathBuf;

    fn signature() -> Signature<'static> {
        Signature::now("Asha Patel", "asha@example.org").expect("signature")
    }

    /// A bare repository with `main` holding one commit, and a clone of it with an identity set.
    fn pair(dir: &Dir) -> (PathBuf, PathBuf) {
        let bare = dir.0.join("remote.git");
        let repo = Repository::init_bare(&bare).expect("bare");
        let blob = repo.blob(b"seed\n").expect("blob");
        let mut builder = repo.treebuilder(None).expect("treebuilder");
        builder.insert("seed.txt", blob, 0o100644).expect("insert");
        let tree = repo
            .find_tree(builder.write().expect("tree"))
            .expect("find");
        let sig = signature();
        repo.commit(Some("refs/heads/main"), &sig, &sig, "seed", &tree, &[])
            .expect("commit");
        repo.set_head("refs/heads/main").expect("head");
        let clone = dir.0.join("clone");
        let cloned = Repository::clone(bare.to_str().expect("path"), &clone).expect("clone");
        let mut config = cloned.config().expect("config");
        config.set_str("user.name", "Asha Patel").expect("name");
        config
            .set_str("user.email", "asha@example.org")
            .expect("email");
        (bare, clone)
    }

    fn colleague_pushes(bare: &std::path::Path, dir: &Dir) {
        let other = dir.0.join("colleague");
        let repo = Repository::clone(bare.to_str().expect("path"), &other).expect("clone");
        std::fs::write(other.join("theirs.txt"), "theirs\n").expect("write");
        let mut index = repo.index().expect("index");
        index
            .add_path(std::path::Path::new("theirs.txt"))
            .expect("add");
        index.write().expect("write");
        let tree = repo
            .find_tree(index.write_tree().expect("tree"))
            .expect("find");
        let parent = repo.head().expect("head").peel_to_commit().expect("commit");
        let sig = signature();
        repo.commit(Some("HEAD"), &sig, &sig, "theirs", &tree, &[&parent])
            .expect("commit");
        repo.find_remote("origin")
            .expect("remote")
            .push(&["refs/heads/main:refs/heads/main"], None)
            .expect("push");
    }

    // Scenario: Git push refused because the remote moved
    #[test]
    fn git_push_refused_because_the_remote_moved() {
        let dir = Dir::new();
        let (bare, clone) = pair(&dir);
        std::fs::write(clone.join("rates.md"), "new rates\n").expect("write");
        // The item is committed, then the remote branch moves before the push.
        let oid = commit_paths(
            &clone,
            Some("main"),
            &["rates.md"],
            "New rates\n\nSent from ordin after approval",
        )
        .expect("commit");
        colleague_pushes(&bare, &dir);
        let repo = Repository::open(&clone).expect("repo");
        let failure = push_refspec(&repo, "origin", "refs/heads/main:refs/heads/main", None)
            .expect_err("refused");
        // Nothing is forced: the remote keeps the colleague's commit, and the local commit is kept.
        assert_eq!(failure, GitFailure::NonFastForward);
        assert_eq!(failure.code(), "PUSH_ORIGIN_CHANGED");
        let remote = Repository::open_bare(&bare).expect("bare");
        let remote_tip = remote.refname_to_id("refs/heads/main").expect("main");
        assert_ne!(remote_tip, oid);
        assert!(repo.find_commit(oid).is_ok(), "the local commit is kept");
        assert_eq!(
            repo.refname_to_id("refs/heads/main").expect("local main"),
            oid
        );
        assert!(
            remote.find_commit(oid).is_err(),
            "the commit never reached the remote"
        );
    }

    #[test]
    fn a_commit_adds_exactly_the_named_path_and_a_push_without_force_succeeds() {
        let dir = Dir::new();
        let (bare, clone) = pair(&dir);
        std::fs::write(clone.join("rates.md"), "new rates\n").expect("write");
        std::fs::write(clone.join("other.md"), "not named\n").expect("write");
        let oid = commit_paths(&clone, Some("main"), &["rates.md"], "New rates").expect("commit");
        let repo = Repository::open(&clone).expect("repo");
        let commit = repo.find_commit(oid).expect("commit");
        let tree = commit.tree().expect("tree");
        assert!(tree.get_name("rates.md").is_some());
        assert!(
            tree.get_name("other.md").is_none(),
            "only the named path is added"
        );
        push_refspec(&repo, "origin", "refs/heads/main:refs/heads/main", None).expect("push");
        assert_eq!(
            Repository::open_bare(&bare)
                .expect("bare")
                .refname_to_id("refs/heads/main")
                .expect("main"),
            oid
        );
    }

    #[test]
    fn a_missing_identity_or_another_branch_stops_before_anything_is_committed() {
        let dir = Dir::new();
        let (_, clone) = pair(&dir);
        std::fs::write(clone.join("rates.md"), "x\n").expect("write");
        assert!(matches!(
            commit_paths(&clone, Some("release"), &["rates.md"], "m"),
            Err(GitStep::WrongBranch)
        ));
        let repo = Repository::open(&clone).expect("repo");
        let mut config = repo.config().expect("config");
        config.set_str("user.name", "").expect("name");
        config.set_str("user.email", "").expect("email");
        assert!(matches!(
            commit_paths(&clone, Some("main"), &["rates.md"], "m"),
            Err(GitStep::IdentityMissing)
        ));
    }

    #[test]
    fn what_the_remote_and_the_transport_say_maps_to_the_outcome_codes() {
        assert_eq!(
            classify_message("rejected: non-fast-forward").map(|f| f.code()),
            Some("PUSH_ORIGIN_CHANGED")
        );
        assert_eq!(
            classify_message("unexpected http status code: 403").map(|f| f.code()),
            Some("PUSH_FORBIDDEN")
        );
        assert_eq!(
            classify_message("operation timed out").map(|f| f.code()),
            Some("PUSH_OUTCOME_UNKNOWN")
        );
        assert_eq!(classify_message("something else"), None);
        let net = git2::Error::new(
            ErrorCode::GenericError,
            ErrorClass::Net,
            "connection refused",
        );
        assert_eq!(classify(&net).code(), "PUSH_ORIGIN_UNREACHABLE");
        let auth = git2::Error::new(ErrorCode::Auth, ErrorClass::Http, "bad credentials");
        assert_eq!(classify(&auth).code(), "PUSH_SIGN_IN_NEEDED");
        let other = git2::Error::new(ErrorCode::GenericError, ErrorClass::Odb, "odd");
        assert_eq!(classify(&other).code(), "PUSH_DESTINATION_REFUSED");
    }

    #[test]
    fn a_fetched_tip_contains_the_commit_it_descends_from() {
        let dir = Dir::new();
        let (bare, clone) = pair(&dir);
        let repo = Repository::open(&clone).expect("repo");
        fetch_branch(&repo, "origin", "main", None).expect("fetch");
        let tip = remote_tip(&repo, "origin", "main").expect("tip");
        let seed = repo.refname_to_id("refs/heads/main").expect("main");
        assert!(contains_commit(&repo, tip, seed));
        colleague_pushes(&bare, &dir);
        fetch_branch(&repo, "origin", "main", None).expect("fetch");
        let moved = remote_tip(&repo, "origin", "main").expect("tip");
        assert!(
            contains_commit(&repo, moved, seed),
            "the moved tip descends from the seed"
        );
        assert!(
            !contains_commit(&repo, seed, moved),
            "the seed does not contain the newer commit"
        );
    }
}
