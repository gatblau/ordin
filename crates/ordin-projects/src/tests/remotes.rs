//! Git against a local bare repository made by the test. `origin` keeps the HTTPS address a real
//! project has, so the host and sign-in rules run; `url.<bare>.insteadOf` carries the traffic to the
//! bare repository, and no network is used. An effective URL that is not https on the expected host
//! is refused, so these tests live in the crate: `Tunables::local_remote`, which exists only under
//! `cfg(test)`, names the one `file://` address that stands in for the host.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use git2::{Repository, Signature};
use ordin_types::{CodingTaskId, PlaceId, ProjectId, RunId};

use super::support::{
    self, commit_all, commit_all_as, fixture, fixture_with, read, repository, write, Fixture,
    SignedIn, GITHUB_TOKEN, NOBODY,
};
use crate::service::lock;
use crate::Project;

const ORIGIN: &str = "https://github.com/acme/supplier-portal.git";

/// A bare repository holding `main` with `rates.md`, and a working clone opened as a project.
struct Remote {
    bare: PathBuf,
    folder: PathBuf,
    place: PlaceId,
    project: ProjectId,
}

fn clone_of(bare: &Path, into: &Path) -> Repository {
    let repo = Repository::clone(bare.to_str().unwrap(), into).unwrap();
    {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Sam Lee").unwrap();
        config.set_str("user.email", "sam@example.org").unwrap();
    }
    // Git for Windows sets `core.autocrlf = true` for the whole computer, and the clone took it;
    // the tests compare bytes, so the files are written again without it.
    #[cfg(windows)]
    {
        repo.config()
            .unwrap()
            .set_bool("core.autocrlf", false)
            .unwrap();
        for entry in repo.index().unwrap().iter() {
            let file = into.join(String::from_utf8_lossy(&entry.path).as_ref());
            std::fs::remove_file(file).unwrap();
        }
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }
    repo
}

fn push(repo: &Repository, refspec: &str) {
    repo.find_remote("origin")
        .unwrap()
        .push(&[refspec], None)
        .unwrap();
}

/// The `file://` address of `path`, with forward slashes: a Windows path has a drive and backslashes.
fn file_url(path: &Path) -> String {
    let text = path.display().to_string();
    // A canonical Windows path starts `\\?\`, which a URL cannot carry.
    let text = text
        .strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

async fn remote_with(f: &Fixture, origin: &str) -> Remote {
    let bare = f.dir.0.join("remote.git");
    let mut init = git2::RepositoryInitOptions::new();
    init.bare(true).initial_head("main");
    Repository::init_opts(&bare, &init).unwrap();
    let seed = f.dir.0.join("seed");
    let start = repository(&seed, &[("rates.md", "first\n"), ("other.md", "x\n")]);
    start.remote("origin", bare.to_str().unwrap()).unwrap();
    push(&start, "refs/heads/main:refs/heads/main");

    let folder = f.dir.0.join("supplier-portal");
    let repo = clone_of(&bare, &folder);
    {
        let mut config = repo.config().unwrap();
        config.set_str("remote.origin.url", origin).unwrap();
        let key = format!("url.{}.insteadOf", file_url(&bare));
        config.set_str(&key, origin).unwrap();
    }
    lock(&f.service.tunables).local_remote = Some(file_url(&bare));
    let place = f.place(&folder).await;
    let project = f.service.open_project(place, f.workspace).await.unwrap();
    Remote {
        bare,
        folder,
        place,
        project: project.id,
    }
}

async fn remote(f: &Fixture) -> Remote {
    remote_with(f, ORIGIN).await
}

/// Sam Lee commits to `branch` of the bare repository from their own clone.
fn teammate_commits(f: &Fixture, r: &Remote, name: &str, branch: &str, text: &str) -> git2::Oid {
    let folder = f.dir.0.join(name);
    let repo = if folder.exists() {
        Repository::open(&folder).unwrap()
    } else {
        clone_of(&r.bare, &folder)
    };
    let target = repo
        .refname_to_id("refs/remotes/origin/main")
        .and_then(|id| repo.find_commit(id))
        .unwrap();
    repo.branch(branch, &target, true).ok();
    repo.set_head(&format!("refs/heads/{branch}")).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    write(&folder, "rates.md", text);
    let sam = Signature::now("Sam Lee", "sam@example.org").unwrap();
    let id = commit_all_as(&repo, "Change the rates", &sam);
    push(&repo, &format!("refs/heads/{branch}:refs/heads/{branch}"));
    id
}

fn head_of(folder: &Path) -> git2::Oid {
    Repository::open(folder)
        .unwrap()
        .head()
        .unwrap()
        .target()
        .unwrap()
}

#[tokio::test]
async fn happy_path_a_merged_change_to_an_original_is_reported() {
    let f = fixture().await;
    let r = remote(&f).await;
    f.service
        .follow_original(r.place, "rates.md", "main")
        .await
        .unwrap();
    let first = f.service.fetch_followed(r.place, "main").await.unwrap();
    assert!(first.changes.is_empty(), "the first fetch records the blob");

    let head_before = head_of(&r.folder);
    let mut changes = f.service.subscribe_original_changes();
    let commit = teammate_commits(&f, &r, "sam", "main", "second\n");
    let outcome = f.service.fetch_followed(r.place, "main").await.unwrap();

    let change = changes.try_recv().expect("broadcast");
    assert_eq!(change.path, "rates.md");
    assert_eq!(change.commit, commit.to_string());
    assert_eq!(change.author, "Sam Lee");
    assert_eq!(change.branch, "main");
    assert_eq!(outcome.changes.len(), 1);
    assert_eq!(outcome.commit, commit.to_string());
    assert_eq!(
        read(&r.folder, "rates.md"),
        "first\n",
        "the working copy stays"
    );
    assert_eq!(head_of(&r.folder), head_before, "HEAD stays");
    let text = f
        .service
        .read_at_commit(r.place, &change.commit, "rates.md")
        .await
        .unwrap();
    assert_eq!(text, b"second\n");

    // Nothing new, nothing reported.
    let again = f.service.fetch_followed(r.place, "main").await.unwrap();
    assert!(again.changes.is_empty());
    assert!(changes.try_recv().is_err());
}

#[tokio::test]
async fn edge_case_a_change_on_another_branch_updates_nothing() {
    let f = fixture().await;
    let r = remote(&f).await;
    f.service
        .follow_original(r.place, "rates.md", "main")
        .await
        .unwrap();
    f.service.fetch_followed(r.place, "main").await.unwrap();
    let mut changes = f.service.subscribe_original_changes();
    teammate_commits(&f, &r, "sam", "draft", "on draft\n");
    let outcome = f.service.fetch_followed(r.place, "main").await.unwrap();
    assert!(outcome.changes.is_empty());
    assert!(changes.try_recv().is_err());
}

#[tokio::test]
async fn a_repository_on_the_company_gitlab_host_uses_the_gitlab_sign_in() {
    let origin = "https://gitlab.acme.co.uk/ops/automation.git";
    let f = fixture_with(SignedIn {
        github: false,
        gitlab: true,
    })
    .await;
    let r = remote_with(&f, origin).await;
    assert!(f.service.fetch_followed(r.place, "main").await.is_ok());

    let f = fixture_with(SignedIn {
        github: true,
        gitlab: false,
    })
    .await;
    let r = remote_with(&f, origin).await;
    let error = f.service.fetch_followed(r.place, "main").await.unwrap_err();
    assert_eq!(error.code(), "PROJ_SIGN_IN_NEEDED");
}

#[tokio::test]
async fn the_engineer_commits_and_pushes_to_the_remote() {
    let f = fixture().await;
    let r = remote(&f).await;
    write(&r.folder, "rates.md", "mine\n");
    let commit = f
        .service
        .commit(r.project, "Update rates", None)
        .await
        .unwrap();
    let pushed = f.service.push(r.project, None).await.unwrap();
    assert_eq!(pushed.commit, commit.commit);
    assert_eq!(pushed.branch, "main");
    let bare = Repository::open_bare(&r.bare).unwrap();
    assert_eq!(
        bare.refname_to_id("refs/heads/main").unwrap().to_string(),
        commit.commit
    );
    assert_eq!(f.entries_of("git_pushed").await.len(), 1);
}

#[tokio::test]
async fn proj_push_refused_when_the_push_is_rejected_and_the_commit_stays() {
    let f = fixture().await;
    let r = remote(&f).await;
    teammate_commits(&f, &r, "sam", "main", "theirs\n");
    write(&r.folder, "rates.md", "mine\n");
    let local = f.service.commit(r.project, "Mine", None).await.unwrap();
    let error = f.service.push(r.project, None).await.unwrap_err();
    assert_eq!(error.code(), "PROJ_PUSH_REFUSED");
    assert!(error.retryable());
    assert_eq!(head_of(&r.folder).to_string(), local.commit);
}

#[tokio::test]
async fn an_ordin_branch_never_leaves_the_computer() {
    let f = fixture().await;
    let r = remote(&f).await;
    let repo = Repository::open(&r.folder).unwrap();
    let head = repo.head().unwrap().peel_to_commit().unwrap();
    repo.branch("ordin/private", &head, false).unwrap();
    repo.set_head("refs/heads/ordin/private").unwrap();
    let error = f.service.push(r.project, None).await.unwrap_err();
    assert_eq!(error.code(), "PROJ_PUSH_REFUSED");
}

#[tokio::test]
async fn remote_state_and_default_branch_read_the_remote() {
    let f = fixture().await;
    let r = remote(&f).await;
    let commit = teammate_commits(&f, &r, "sam", "main", "newer\n");
    let state = f.service.remote_state(r.place, "main").await.unwrap();
    assert_eq!(state.commit, commit.to_string());
    assert_eq!(f.service.default_branch(r.place).await.unwrap(), "main");
}

#[tokio::test]
async fn a_fetch_with_no_sign_in_records_the_error() {
    let f = fixture_with(NOBODY).await;
    let r = remote(&f).await;
    f.service
        .follow_original(r.place, "rates.md", "main")
        .await
        .unwrap();
    let error = f.service.fetch_followed(r.place, "main").await.unwrap_err();
    assert_eq!(error.code(), "PROJ_SIGN_IN_NEEDED");
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM repository_fetch_state WHERE last_error = 'PROJ_SIGN_IN_NEEDED'"
        )
        .await,
        1
    );
    f.service.workspace_opened(&[r.place]).await;
    f.service
        .unfollow_original(r.place, "rates.md", "main")
        .await
        .unwrap();
    assert_eq!(f.count("SELECT COUNT(*) FROM followed_originals").await, 0);
}

#[tokio::test]
async fn workspace_opened_fetches_its_followed_repositories() {
    let f = fixture().await;
    let r = remote(&f).await;
    f.service
        .follow_original(r.place, "rates.md", "main")
        .await
        .unwrap();
    f.service.workspace_opened(&[PlaceId::new()]).await;
    assert_eq!(
        f.count("SELECT COUNT(*) FROM repository_fetch_state").await,
        0
    );
    f.service.workspace_opened(&[r.place]).await;
    assert_eq!(
        f.count("SELECT COUNT(*) FROM repository_fetch_state WHERE remote_commit IS NOT NULL")
            .await,
        1
    );
}

#[tokio::test]
async fn start_runs_the_first_fetch_of_every_followed_original() {
    let f = fixture().await;
    let r = remote(&f).await;
    f.service
        .follow_original(r.place, "rates.md", "main")
        .await
        .unwrap();
    f.service.start().await.unwrap();
    let mut fetched = 0;
    for _ in 0..200 {
        fetched = f
            .count("SELECT COUNT(*) FROM followed_originals WHERE last_blob IS NOT NULL")
            .await;
        if fetched == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    f.service.shutdown().await;
    assert_eq!(fetched, 1);
}

#[tokio::test]
async fn a_coding_task_commits_nothing_to_the_remote() {
    let f = fixture().await;
    let r = remote(&f).await;
    let project = f.service.project(r.project).await.unwrap();
    let started = f.start_task(&project, "rates").await;
    let worktree = support::worktree(&started.task);
    write(&worktree, "rates.md", "agent\n");
    let repo = Repository::open(&worktree).unwrap();
    commit_all(&repo, "Agent commit in the worktree");
    let set = f
        .service
        .end_coding_run(started.task.id, RunId::new())
        .await
        .unwrap();
    assert_eq!(
        set.entries.len(),
        1,
        "a commit in the worktree is still a change"
    );
    let bare = Repository::open_bare(&r.bare).unwrap();
    assert!(bare
        .find_branch("ordin/rates", git2::BranchType::Local)
        .is_err());
    let _: (&Project, CodingTaskId) = (&project, started.task.id);
}

/// A loopback stand-in for a host the token must not reach: it counts connections, keeps each
/// request's head and answers `401`, which is what makes a transport ask for credentials.
struct Trap {
    url: String,
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<String>>>,
}

fn trap() -> Trap {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (counted, kept) = (connections.clone(), requests.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            counted.fetch_add(1, Ordering::SeqCst);
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
                head.push(byte[0]);
            }
            kept.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&head).to_lowercase());
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\n\
                  Content-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    Trap {
        url,
        connections,
        requests,
    }
}

/// `origin` as configured still reads `github.com`, while `insteadOf` leads somewhere else.
fn lead_origin_to(r: &Remote, target: &str) {
    let mut config = Repository::open(&r.folder).unwrap().config().unwrap();
    let carried = format!("url.{}.insteadOf", file_url(&r.bare));
    config.remove(&carried).unwrap();
    config
        .set_str(&format!("url.{target}.insteadOf"), ORIGIN)
        .unwrap();
}

fn unreachable_code<T>(result: Result<T, crate::ProjectError>) -> &'static str {
    result.err().map_or("no error", |error| error.code())
}

#[tokio::test]
async fn an_instead_of_rewrite_to_another_host_is_refused_before_any_connection() {
    let f = fixture().await;
    let r = remote(&f).await;
    let host = trap();
    lead_origin_to(&r, &host.url);
    let code = "PROJ_REMOTE_UNREACHABLE";
    assert_eq!(
        unreachable_code(f.service.fetch_followed(r.place, "main").await),
        code
    );
    assert_eq!(
        unreachable_code(f.service.remote_state(r.place, "main").await),
        code
    );
    assert_eq!(
        unreachable_code(f.service.default_branch(r.place).await),
        code
    );
    write(&r.folder, "rates.md", "mine\n");
    f.service.commit(r.project, "Mine", None).await.unwrap();
    assert_eq!(
        unreachable_code(f.service.push(r.project, None).await),
        code
    );
    assert_eq!(host.connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_push_url_to_another_host_is_refused_before_any_connection() {
    let f = fixture().await;
    let r = remote(&f).await;
    let host = trap();
    Repository::open(&r.folder)
        .unwrap()
        .config()
        .unwrap()
        .set_str("remote.origin.pushurl", &format!("{}acme.git", host.url))
        .unwrap();
    write(&r.folder, "rates.md", "mine\n");
    f.service.commit(r.project, "Mine", None).await.unwrap();
    assert_eq!(
        unreachable_code(f.service.push(r.project, None).await),
        "PROJ_REMOTE_UNREACHABLE"
    );
    assert_eq!(host.connections.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_token_is_withheld_from_a_url_the_credentials_callback_does_not_trust() {
    let f = fixture().await;
    let r = remote(&f).await;
    let host = trap();
    let url = format!("{}acme/portal.git", host.url);
    Repository::open(&r.folder)
        .unwrap()
        .config()
        .unwrap()
        .set_str("remote.origin.url", &url)
        .unwrap();
    // The check before the connection is passed on purpose, as a redirect would pass it.
    let auth = crate::git::NetAuth {
        username: "x-access-token",
        token: GITHUB_TOKEN.to_owned(),
        host: "github.com".to_owned(),
        local_remote: Some(url),
        test_address: None,
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let folder = r.folder.clone();
    let result =
        crate::git::blocking(move || crate::git::remote_default_branch(&folder, &auth, stop)).await;
    assert_eq!(unreachable_code(result), "PROJ_REMOTE_UNREACHABLE");
    assert!(
        host.connections.load(Ordering::SeqCst) > 0,
        "it did connect"
    );
    let requests = host.requests.lock().unwrap();
    assert!(
        requests.iter().all(|head| !head.contains("authorization")),
        "{requests:?}"
    );
}

// PROJ-SEAM-2: a remote under the test host's address is a GitHub remote reached over loopback
// http, and the test's token, not Identity's, answers its challenge.
// spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
#[tokio::test]
async fn a_test_host_takes_the_push_with_its_own_token() {
    use base64::Engine as _;
    let f = fixture_with(NOBODY).await;
    let r = remote(&f).await;
    let host = trap();
    let url = format!("{}acme/portal.git", host.url);
    {
        let mut config = Repository::open(&r.folder).unwrap().config().unwrap();
        config.set_str("remote.origin.url", &url).unwrap();
        config
            .remove(&format!("url.{}.insteadOf", file_url(&r.bare)))
            .unwrap();
    }
    write(&r.folder, "rates.md", "mine\n");
    f.service.commit(r.project, "Mine", None).await.unwrap();
    // Without the seam the loopback address is no repository host.
    assert_eq!(
        unreachable_code(f.service.push(r.project, None).await),
        "PROJ_REMOTE_UNREACHABLE"
    );
    assert_eq!(host.connections.load(Ordering::SeqCst), 0);

    f.service
        .use_test_host(&host.url, "seam-token")
        .expect("a loopback host");
    // The trap refuses every request, so the push fails once the token has been offered.
    assert!(f.service.push(r.project, None).await.is_err());
    let offered = base64::engine::general_purpose::STANDARD.encode("x-access-token:seam-token");
    let requests = host.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|head| head.contains(&format!("authorization: basic {}", offered.to_lowercase()))),
        "{requests:?}"
    );
}

// spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
#[tokio::test]
async fn error_path_a_test_host_off_loopback_over_http_is_refused() {
    let f = fixture().await;
    let refused = f
        .service
        .use_test_host("http://git.example/", "seam-token")
        .unwrap_err();
    assert_eq!(refused.code(), "PROJ_REMOTE_UNREACHABLE");
    assert!(lock(&f.service.tunables).test_host.is_none());
}
