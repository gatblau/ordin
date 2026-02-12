//! GitHub over SSH with a key the person chose (ProjectService.md PROJ-SSH-1 to PROJ-SSH-5; amended
//! January 2026; user decision; Approved by the user January 2026).
//!
//! The person may give ordin an SSH key in place of, or beside, a GitHub token. Only git's own traffic
//! (fetch, push, the remote's default branch) can use it; the GitHub REST API still needs the token.
//! The key is named by its path only: this module never opens, reads or logs the private key. libgit2
//! receives the path and reads the file itself. The public half (`<key>.pub`) is read, because it is
//! shown to the person to paste into GitHub.
//!
//! GitHub's host key is checked against the fingerprints GitHub publishes, built in here, never against
//! the person's `known_hosts`, so a first connection from a new computer works and a stand-in host is
//! refused.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use git2::{CertificateCheckStatus, Cred, CredentialType, ErrorClass, ErrorCode, RemoteCallbacks};
use sha2::{Digest, Sha256};

/// The one host an SSH remote may name.
pub const GITHUB_HOST: &str = "github.com";

/// GitHub's SSH service on port 443, for networks that block port 22; it presents the same host keys
/// (PROJ-SSH-7).
pub const GITHUB_SSH_443_HOST: &str = "ssh.github.com";

/// How long one port probe of PROJ-SSH-7 waits for the connection.
pub const PORT_PROBE_LIMIT: Duration = Duration::from_secs(3);

/// How long a probe's answer stands before the next Git operation probes again (a laptop changes networks).
pub const PORT_PROBE_FRESH: Duration = Duration::from_secs(600);

/// Which of GitHub's SSH addresses git's traffic uses (PROJ-SSH-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshPort {
    /// `github.com` on port 22, the default.
    Standard,
    /// `ssh.github.com` on port 443, when port 22 does not answer and 443 does.
    Https,
}

/// The last probe's answer and when it was made; none until a probe ran.
static PORT: RwLock<Option<(SshPort, Instant)>> = RwLock::new(None);

/// The address the last probe chose; [`SshPort::Standard`] before any probe.
pub fn current_port() -> SshPort {
    PORT.read()
        .unwrap_or_else(|p| p.into_inner())
        .map_or(SshPort::Standard, |(port, _)| port)
}

/// PROJ-SSH-7: port 22 when it answers, else port 443 when that answers, else port 22 (the failure is then
/// the connection's own). `open` says whether the host and port send an SSH greeting.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-7
pub fn choose_port_with(open: impl Fn(&str, u16) -> bool) -> SshPort {
    let port = pick_port(open);
    if port == SshPort::Https {
        tracing::info!("ssh.port_443_used");
    }
    *PORT.write().unwrap_or_else(|p| p.into_inner()) = Some((port, Instant::now()));
    port
}

/// The choice of [`choose_port_with`], without remembering it.
pub fn pick_port(open: impl Fn(&str, u16) -> bool) -> SshPort {
    if open(GITHUB_HOST, 22) {
        SshPort::Standard
    } else if open(GITHUB_SSH_443_HOST, 443) {
        SshPort::Https
    } else {
        SshPort::Standard
    }
}

/// Whether `host:port` sends an SSH greeting (`SSH-`) within [`PORT_PROBE_LIMIT`]. Nothing is sent. A
/// connection that opens but stays silent counts as closed: some company networks let the TCP connection
/// to port 22 open and then stop SSH itself (the 0.2.1-test.19 test Mac).
fn tcp_open(host: &str, port: u16) -> bool {
    use std::io::Read;
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs.into_iter().any(|addr| {
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, PORT_PROBE_LIMIT) else {
            return false;
        };
        let mut greeting = [0u8; 4];
        stream.set_read_timeout(Some(PORT_PROBE_LIMIT)).is_ok()
            && stream.read_exact(&mut greeting).is_ok()
            && &greeting == b"SSH-"
    })
}

/// Probes again when no probe ran or the last one is older than [`PORT_PROBE_FRESH`].
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-7
pub fn refresh_port_if_stale() {
    let stale = PORT
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .is_none_or(|(_, at)| at.elapsed() >= PORT_PROBE_FRESH);
    if stale {
        choose_port_with(tcp_open);
    }
}

/// The SHA-256 fingerprints of GitHub's SSH host keys as GitHub publishes them: RSA
/// `uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s`, Ed25519
/// `+DiY3wvvV6TuJJhbpZisF/zLDA0zPMSvHdkr4UvCOqU` and ECDSA
/// `p2QAMXNIC1TJYWeIOttrVc98/R1BUFWu3/LiyKgUfQM`.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-2
pub const GITHUB_HOST_KEY_SHA256: [[u8; 32]; 3] = [
    [
        0xb8, 0xd8, 0x95, 0xce, 0xd9, 0x2c, 0x0a, 0xc0, 0xe1, 0x71, 0xcd, 0x2e, 0xf5, 0xef, 0x01,
        0xba, 0x34, 0x17, 0x55, 0x4a, 0x4a, 0x64, 0x80, 0xd3, 0x31, 0xcc, 0xc2, 0xbe, 0x3d, 0xed,
        0x0f, 0x6b,
    ],
    [
        0xf8, 0x38, 0x98, 0xdf, 0x0b, 0xef, 0x57, 0xa4, 0xee, 0x24, 0x98, 0x5b, 0xa5, 0x98, 0xac,
        0x17, 0xfc, 0xcb, 0x0c, 0x0d, 0x33, 0x3c, 0xc4, 0xaf, 0x1d, 0xd9, 0x2b, 0xe1, 0x4b, 0xc2,
        0x3a, 0xa5,
    ],
    [
        0xa7, 0x64, 0x00, 0x31, 0x73, 0x48, 0x0b, 0x54, 0xc9, 0x61, 0x67, 0x88, 0x3a, 0xdb, 0x6b,
        0x55, 0xcf, 0x7c, 0xfd, 0x1d, 0x41, 0x50, 0x55, 0xae, 0xdf, 0xf2, 0xe2, 0xc8, 0xa8, 0x14,
        0x7d, 0x03,
    ],
];

/// The public repository "Check it works" connects to: any GitHub repository answers an SSH connection
/// only once the key is accepted.
pub const CHECK_REPOSITORY: &str = "github/gitignore";

/// The name of a key ordin makes, in `~/.ssh`; a second one gets `_2`, and so on.
pub const NEW_KEY_NAME: &str = "ordin_github_ed25519";

/// The key the person chose, set by the program at start and on every change of the choice.
static CHOSEN: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Sets, or with `None` clears, the key git's GitHub traffic uses.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
pub fn set_chosen_key(key: Option<PathBuf>) {
    *CHOSEN.write().unwrap_or_else(|p| p.into_inner()) = key;
}

/// The key git's GitHub traffic uses, if the person chose one.
pub fn chosen_key() -> Option<PathBuf> {
    CHOSEN.read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// `owner/name` is one segment, a `/`, and one segment, each of letters, digits, `.`, `-` and `_`,
/// neither starting with `.` or `-`.
fn plain_repository(path: &str) -> bool {
    let mut parts = path.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    [owner, name].iter().all(|s| {
        !s.is_empty()
            && !s.starts_with(['.', '-'])
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    })
}

fn without_git(path: &str) -> &str {
    path.strip_suffix(".git").unwrap_or(path)
}

/// `owner/name` of a GitHub SSH address: `git@github.com:owner/name[.git]` or
/// `ssh://git@github.com/owner/name[.git]`, or GitHub's port-443 form
/// `ssh://git@ssh.github.com:443/owner/name[.git]` (PROJ-SSH-7), the host compared without regard to
/// case. Any other user, host, port, or path is `None`.
// spec: ordin-desktop/components/ProjectService#SA-CLOSE-01
pub fn github_ssh_repository(url: &str) -> Option<String> {
    let rest = if let Some(rest) = url.strip_prefix("ssh://git@") {
        let (host, path) = rest.split_once('/')?;
        let port_443 = host
            .strip_suffix(":443")
            .is_some_and(|name| name.eq_ignore_ascii_case(GITHUB_SSH_443_HOST));
        (host.eq_ignore_ascii_case(GITHUB_HOST) || port_443).then_some(path)?
    } else {
        let rest = url.strip_prefix("git@")?;
        let (host, path) = rest.split_once(':')?;
        host.eq_ignore_ascii_case(GITHUB_HOST).then_some(path)?
    };
    let path = without_git(rest);
    plain_repository(path).then(|| path.to_owned())
}

/// `owner/name` of a GitHub address in any form ordin accepts: the SSH forms above, or
/// `https://github.com/owner/name[.git]` with no user, port, query or fragment.
pub fn github_repository(url: &str) -> Option<String> {
    if let Some(found) = github_ssh_repository(url) {
        return Some(found);
    }
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !host.eq_ignore_ascii_case(GITHUB_HOST) {
        return None;
    }
    let path = without_git(path.strip_suffix('/').unwrap_or(path));
    plain_repository(path).then(|| path.to_owned())
}

/// The SSH address of a GitHub repository.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-7
pub fn github_ssh_url(repository: &str) -> String {
    ssh_url_for(current_port(), repository)
}

/// The SSH address of `repository` on `port`'s host.
pub fn ssh_url_for(port: SshPort, repository: &str) -> String {
    match port {
        SshPort::Standard => format!("git@{GITHUB_HOST}:{repository}.git"),
        SshPort::Https => format!("ssh://git@{GITHUB_SSH_443_HOST}:443/{repository}.git"),
    }
}

/// Whether `host` is one of GitHub's two SSH host names.
fn github_ssh_host(host: &str) -> bool {
    host.eq_ignore_ascii_case(GITHUB_HOST) || host.eq_ignore_ascii_case(GITHUB_SSH_443_HOST)
}

/// The SSH address and the key to use for `url`, when it is a GitHub repository and the person chose a
/// key; `None` otherwise, so the caller goes on over HTTPS with the token.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-1
pub fn github_route(url: &str) -> Option<(String, PathBuf)> {
    let key = chosen_key();
    if key.is_some() && github_repository(url).is_some() {
        refresh_port_if_stale();
    }
    route_with(url, key)
}

/// [`github_route`] with the key given, for callers and tests that hold it.
pub fn route_with(url: &str, key: Option<PathBuf>) -> Option<(String, PathBuf)> {
    let key = key?;
    let repository = github_repository(url)?;
    Some((github_ssh_url(&repository), key))
}

/// Whether the host key libgit2 presents is one of GitHub's (PROJ-SSH-2).
pub fn github_host_key(cert: &git2::cert::Cert<'_>) -> bool {
    let Some(key) = cert.as_hostkey() else {
        return false;
    };
    let digest: Option<[u8; 32]> = key
        .hash_sha256()
        .copied()
        .or_else(|| key.hostkey().map(|raw| Sha256::digest(raw).into()));
    digest.is_some_and(|d| GITHUB_HOST_KEY_SHA256.contains(&d))
}

/// The public half of `key`: `<key>.pub`.
pub fn public_path(key: &Path) -> PathBuf {
    let mut name = key.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

/// Adds the SSH credentials and GitHub's host-key check to `callbacks`. The key goes to libgit2 by
/// path, once; a second request (the key refused) fails. A certificate that is not an SSH host key
/// (HTTPS) is left to libgit2's own check.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-2
pub fn add_callbacks<'a>(callbacks: &mut RemoteCallbacks<'a>, key: &'a Path) {
    let mut asked = 0u8;
    callbacks.credentials(move |_url, _user, allowed| {
        asked += 1;
        if asked > 1 || !allowed.contains(CredentialType::SSH_KEY) {
            return Err(git2::Error::new(
                ErrorCode::Auth,
                ErrorClass::Ssh,
                "the key was not accepted",
            ));
        }
        let public = public_path(key);
        Cred::ssh_key(
            "git",
            public.is_file().then_some(public.as_path()),
            key,
            None,
        )
    });
    callbacks.certificate_check(|cert, host| {
        if cert.as_hostkey().is_none() {
            return Ok(CertificateCheckStatus::CertificatePassthrough);
        }
        if github_ssh_host(host) && github_host_key(cert) {
            Ok(CertificateCheckStatus::CertificateOk)
        } else {
            // spec: ordin-desktop/components/ProjectService#PROJ-SSH-6
            tracing::warn!(
                github = github_ssh_host(host),
                key_type =
                    format!("{:?}", cert.as_hostkey().and_then(|k| k.hostkey_type())).as_str(),
                "ssh.host_key_refused"
            );
            Err(git2::Error::new(
                ErrorCode::Certificate,
                ErrorClass::Ssh,
                "the host key is not GitHub's",
            ))
        }
    });
}

/// New callbacks with only [`add_callbacks`].
pub fn callbacks(key: &Path) -> RemoteCallbacks<'_> {
    let mut callbacks = RemoteCallbacks::new();
    add_callbacks(&mut callbacks, key);
    callbacks
}

// ---- the keys in ~/.ssh ---------------------------------------------------------------------------

/// One key in the SSH folder: the private file's path, its file name, and the public line to paste
/// into GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshKeyFile {
    pub path: PathBuf,
    pub name: String,
    pub public_key: String,
}

/// The first line of a public key file, when it looks like one (`ssh-…`, `ecdsa-…` or `sk-…` and a
/// base64 body). Files over 16 KiB are not public keys and are not read.
pub fn read_public_key(public: &Path) -> Option<String> {
    let meta = std::fs::metadata(public).ok()?;
    if !meta.is_file() || meta.len() > 16 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(public).ok()?;
    let line = text.lines().next()?.trim();
    let mut words = line.split_whitespace();
    let kind = words.next()?;
    let body = words.next()?;
    let known = kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-");
    (known && !body.is_empty()).then(|| line.to_owned())
}

/// The keys in `folder`: every `*.pub` whose private file sits beside it, by name. The private files
/// are only checked to exist.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-3
pub fn list_keys(folder: &Path) -> Vec<SshKeyFile> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut keys: Vec<SshKeyFile> = entries
        .flatten()
        .filter_map(|entry| {
            let public = entry.path();
            let name = public
                .file_name()?
                .to_str()?
                .strip_suffix(".pub")?
                .to_owned();
            let private = folder.join(&name);
            if name.is_empty() || !private.is_file() {
                return None;
            }
            let public_key = read_public_key(&public)?;
            Some(SshKeyFile {
                path: private,
                name,
                public_key,
            })
        })
        .collect();
    keys.sort_by(|a, b| a.name.cmp(&b.name));
    keys
}

/// Why a key could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeygenError {
    /// No `ssh-keygen` on this computer.
    Missing,
    /// `ssh-keygen` ran and made no key, or the folder could not be made.
    Failed,
}

/// A free name for a new key in `folder`: [`NEW_KEY_NAME`], then `_2`, `_3`, and so on.
fn free_name(folder: &Path) -> PathBuf {
    let mut n = 1u32;
    loop {
        let name = if n == 1 {
            NEW_KEY_NAME.to_owned()
        } else {
            format!("{NEW_KEY_NAME}_{n}")
        };
        let path = folder.join(&name);
        if !path.exists() && !public_path(&path).exists() {
            return path;
        }
        n += 1;
    }
}

/// Makes a new ed25519 key in `folder` (created with mode 0700 when absent) with `program`
/// (`ssh-keygen`), no passphrase, and the comment `ordin`. No pure-Rust crate in the tree writes the
/// OpenSSH key format, so the computer's `ssh-keygen` makes it.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-4
pub fn create_key(folder: &Path, program: &Path) -> Result<SshKeyFile, KeygenError> {
    if !folder.is_dir() {
        std::fs::create_dir_all(folder).map_err(|_| KeygenError::Failed)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700));
        }
    }
    let path = free_name(folder);
    let run = || {
        Command::new(program)
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "ordin", "-f"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    // A program file another process has just written can be briefly busy to start; three tries
    // 50 ms apart cover it.
    let mut status = run();
    for _ in 0..2 {
        match &status {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(50));
                status = run();
            }
            _ => break,
        }
    }
    match status {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(KeygenError::Missing),
        Err(_) => Err(KeygenError::Failed),
        Ok(s) if !s.success() => Err(KeygenError::Failed),
        Ok(_) => {
            let public_key = read_public_key(&public_path(&path)).ok_or(KeygenError::Failed)?;
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(NEW_KEY_NAME)
                .to_owned();
            Ok(SshKeyFile {
                path,
                name,
                public_key,
            })
        }
    }
}

// ---- Check it works -------------------------------------------------------------------------------

/// What "Check it works" found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// GitHub accepted the key.
    Works,
    /// GitHub answered and did not accept the key (not added to GitHub yet, or it has a passphrase).
    Refused,
    /// GitHub could not be reached in time, or the host was not GitHub.
    Unreachable,
}

/// How a failed connection reads. The failure's code and class are logged; its message is not, as it
/// can name a path.
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-6
fn outcome_of(error: &git2::Error) -> CheckOutcome {
    tracing::warn!(
        code = format!("{:?}", error.code()).as_str(),
        class = format!("{:?}", error.class()).as_str(),
        "ssh.check_failed"
    );
    if error.code() == ErrorCode::Auth
        || error.message().contains("authenticat")
        || error.message().contains("the key was not accepted")
    {
        CheckOutcome::Refused
    } else {
        CheckOutcome::Unreachable
    }
}

/// Connects to [`CHECK_REPOSITORY`] over SSH with `key`, as `git ls-remote` would, and reads the answer.
/// The connection runs on its own thread; past `limit` the answer is [`CheckOutcome::Unreachable`].
// spec: ordin-desktop/components/ProjectService#PROJ-SSH-5
pub fn check_key(key: &Path, limit: Duration) -> CheckOutcome {
    let (send, receive) = std::sync::mpsc::channel();
    let key = key.to_path_buf();
    std::thread::spawn(move || {
        let outcome = (|| {
            // PROJ-SSH-7: each check probes again, so a changed network is picked up at once.
            choose_port_with(tcp_open);
            let mut remote = git2::Remote::create_detached(github_ssh_url(CHECK_REPOSITORY))
                .map_err(|e| outcome_of(&e))?;
            remote
                .connect_auth(git2::Direction::Fetch, Some(callbacks(&key)), None)
                .map_err(|e| outcome_of(&e))?;
            let _ = remote.disconnect();
            Ok::<_, CheckOutcome>(CheckOutcome::Works)
        })()
        .unwrap_or_else(|failed| failed);
        let _ = send.send(outcome);
    });
    receive.recv_timeout(limit).unwrap_or_else(|_| {
        // spec: ordin-desktop/components/ProjectService#PROJ-SSH-6
        tracing::warn!(limit_ms = limit.as_millis() as u64, "ssh.check_timed_out");
        CheckOutcome::Unreachable
    })
}

#[cfg(test)]
mod ssh_tests {
    use super::*;

    // PROJ-SSH-7: port 22 when it answers, else 443 when it answers, else 22.
    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-7
    #[test]
    fn table_which_github_address_is_used() {
        for (open_22, open_443, expected) in [
            (true, true, SshPort::Standard),
            (true, false, SshPort::Standard),
            (false, true, SshPort::Https),
            (false, false, SshPort::Standard),
        ] {
            let chosen = pick_port(|host, port| match (host, port) {
                (GITHUB_HOST, 22) => open_22,
                (GITHUB_SSH_443_HOST, 443) => open_443,
                _ => panic!("unexpected probe {host}:{port}"),
            });
            assert_eq!(chosen, expected, "22 open {open_22}, 443 open {open_443}");
        }
        assert_eq!(
            ssh_url_for(SshPort::Standard, "acme/docs"),
            "git@github.com:acme/docs.git"
        );
        assert_eq!(
            ssh_url_for(SshPort::Https, "acme/docs"),
            "ssh://git@ssh.github.com:443/acme/docs.git"
        );
        assert!(github_ssh_host("ssh.github.com") && github_ssh_host("GitHub.com"));
        assert!(!github_ssh_host("ssh.github.com.example.net"));
        // A clone made over port 443 keeps that address as its origin, and it still reads as GitHub's.
        assert_eq!(
            github_ssh_repository("ssh://git@ssh.github.com:443/acme/docs.git").as_deref(),
            Some("acme/docs")
        );
        assert_eq!(
            github_ssh_repository("ssh://git@ssh.github.com:22/acme/docs.git"),
            None
        );
        assert_eq!(
            github_ssh_repository("ssh://git@ssh.github.com/acme/docs.git"),
            None
        );
    }

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let path = std::env::temp_dir().join(format!(
                "ordin-ssh-{tag}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&path).expect("temp dir");
            Dir(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn table_github_ssh_addresses_and_nothing_else() {
        let cases: &[(&str, Option<&str>)] = &[
            ("git@github.com:acme/docs.git", Some("acme/docs")),
            ("git@GitHub.com:acme/docs", Some("acme/docs")),
            ("ssh://git@github.com/acme/docs.git", Some("acme/docs")),
            ("ssh://git@github.com:22/acme/docs.git", None),
            ("ssh://root@github.com/acme/docs.git", None),
            ("git@evil.example:acme/docs.git", None),
            ("git@github.com.evil.example:acme/docs.git", None),
            ("git@github.com:acme/docs/more.git", None),
            ("git@github.com:-acme/docs.git", None),
            ("git@github.com:acme/../docs.git", None),
            ("https://github.com/acme/docs.git", None),
        ];
        for (url, want) in cases {
            assert_eq!(github_ssh_repository(url).as_deref(), *want, "{url}");
        }
        assert_eq!(
            github_repository("https://github.com/acme/docs.git").as_deref(),
            Some("acme/docs")
        );
        assert_eq!(github_repository("https://user@github.com/acme/docs"), None);
        assert_eq!(github_repository("https://gitlab.com/acme/docs"), None);
    }

    #[test]
    fn a_chosen_key_routes_github_over_ssh_and_leaves_other_hosts_alone() {
        let key = PathBuf::from("/home/sam/.ssh/id_ed25519");
        assert_eq!(
            route_with("https://github.com/acme/docs.git", Some(key.clone())),
            Some(("git@github.com:acme/docs.git".to_owned(), key.clone()))
        );
        assert_eq!(
            route_with("https://gitlab.com/acme/docs.git", Some(key)),
            None
        );
        assert_eq!(route_with("https://github.com/acme/docs.git", None), None);
    }

    #[test]
    fn keys_are_listed_from_their_public_halves_only_when_the_private_file_exists() {
        let dir = Dir::new("list");
        std::fs::write(dir.0.join("id_ed25519"), "PRIVATE").unwrap();
        std::fs::write(
            dir.0.join("id_ed25519.pub"),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI sam@laptop\n",
        )
        .unwrap();
        std::fs::write(dir.0.join("orphan.pub"), "ssh-rsa AAAAB3 x\n").unwrap();
        std::fs::write(dir.0.join("notes"), "x").unwrap();
        std::fs::write(dir.0.join("notes.pub"), "not a key\n").unwrap();
        let keys = list_keys(&dir.0);
        assert_eq!(
            keys,
            vec![SshKeyFile {
                path: dir.0.join("id_ed25519"),
                name: "id_ed25519".to_owned(),
                public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI sam@laptop".to_owned(),
            }]
        );
        assert!(list_keys(&dir.0.join("absent")).is_empty());
    }

    /// A stand-in `ssh-keygen` that records its arguments and writes the two files `-f` names, so the
    /// test does not depend on the computer's tool (which refuses to run for a user with no account,
    /// as in a container).
    #[cfg(unix)]
    fn stand_in_keygen(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let program = dir.join("ssh-keygen");
        let log = dir.join("args.log");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\nwhile [ $# -gt 0 ]; do [ \"$1\" = -f ] && f=\"$2\"; shift; done\n\
                 printf 'PRIVATE\\n' > \"$f\"\nprintf 'ssh-ed25519 AAAAC3Nz ordin\\n' > \"$f.pub\"\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        program
    }

    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-4
    #[cfg(unix)]
    #[test]
    fn a_new_key_is_made_with_ed25519_and_no_passphrase_in_a_fresh_ssh_folder_of_a_temporary_home()
    {
        let home = Dir::new("stand-in");
        let program = stand_in_keygen(&home.0);
        let folder = home.0.join(".ssh");
        let made = create_key(&folder, &program).expect("made");
        assert_eq!(made.name, NEW_KEY_NAME);
        assert_eq!(made.path, folder.join(NEW_KEY_NAME));
        assert_eq!(made.public_key, "ssh-ed25519 AAAAC3Nz ordin");
        let args = std::fs::read_to_string(home.0.join("args.log")).unwrap();
        assert!(args.starts_with("-q -t ed25519 -N  -C ordin -f "), "{args}");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        let second = create_key(&folder, &program).expect("second");
        assert_eq!(second.name, format!("{NEW_KEY_NAME}_2"));
        let names: Vec<String> = list_keys(&folder).into_iter().map(|k| k.name).collect();
        assert_eq!(
            names,
            vec![NEW_KEY_NAME.to_owned(), format!("{NEW_KEY_NAME}_2")]
        );
    }

    /// The computer's own `ssh-keygen`, when it is there and runs for this user.
    #[test]
    fn the_computers_ssh_keygen_makes_a_real_key_when_it_can() {
        let home = Dir::new("make");
        let folder = home.0.join(".ssh");
        let Ok(made) = create_key(&folder, Path::new("ssh-keygen")) else {
            return;
        };
        assert_eq!(made.name, NEW_KEY_NAME);
        assert!(made.public_key.starts_with("ssh-ed25519 "));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        let second = create_key(&folder, Path::new("ssh-keygen")).expect("second");
        assert_eq!(second.name, format!("{NEW_KEY_NAME}_2"));
        assert_eq!(list_keys(&folder).len(), 2);
    }

    #[test]
    fn a_missing_ssh_keygen_says_so() {
        let home = Dir::new("missing");
        assert_eq!(
            create_key(&home.0, Path::new("/nonexistent/ssh-keygen-ordin")),
            Err(KeygenError::Missing)
        );
    }

    #[test]
    fn public_path_adds_pub() {
        assert_eq!(
            public_path(Path::new("/a/id_ed25519")),
            PathBuf::from("/a/id_ed25519.pub")
        );
    }

    #[derive(Clone, Default)]
    struct Lines(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lines").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // spec: ordin-desktop/components/ProjectService#PROJ-SSH-6
    #[test]
    fn proj_ssh_6_a_failed_check_logs_its_code_and_class_but_not_its_message() {
        let lines = Lines::default();
        let writer = lines.clone();
        // ordin's own log format, where the message is the event name (Logging.md R-2).
        let subscriber = ordin_types::log::subscriber(move || writer.clone(), tracing::Level::INFO);
        let error = git2::Error::new(
            ErrorCode::GenericError,
            ErrorClass::Ssh,
            "failed to read /home/sam/.ssh/id_ed25519",
        );
        let outcome = tracing::subscriber::with_default(subscriber, || outcome_of(&error));
        assert_eq!(outcome, CheckOutcome::Unreachable);
        let text = String::from_utf8(lines.0.lock().expect("lines").clone()).expect("utf-8");
        assert!(text.contains(r#""event":"ssh.check_failed""#), "{text}");
        assert!(!text.contains("invalid_event_name"), "{text}");
        assert!(
            text.contains("GenericError") && text.contains("Ssh"),
            "{text}"
        );
        assert!(!text.contains("id_ed25519"), "the message leaked: {text}");
    }
}
