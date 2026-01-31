//! A. The local channel: a Unix domain socket on macOS and Linux, a named pipe on Windows. One
//! task accepts; each connection gets its own short-lived task (C).

use std::sync::Arc;

use crate::error::HookRelayError;
use crate::relay::{ActiveGuard, HookRelay};
use crate::types::HookRelayConfig;

// ---- Unix

/// `<app data root>/run`.
#[cfg(unix)]
fn socket_dir(config: &HookRelayConfig) -> std::path::PathBuf {
    config.app_data_root.join("run")
}

#[cfg(unix)]
fn socket_path(config: &HookRelayConfig) -> std::path::PathBuf {
    socket_dir(config).join("hook.sock")
}

/// The longest socket path the operating system takes, in bytes (A.1).
#[cfg(unix)]
const fn path_limit() -> usize {
    if cfg!(target_os = "macos") {
        104
    } else {
        108
    }
}

#[cfg(unix)]
pub(crate) fn endpoint_of(config: &HookRelayConfig) -> String {
    socket_path(config).to_string_lossy().into_owned()
}

#[cfg(unix)]
pub(crate) fn check_endpoint_length(config: &HookRelayConfig) -> Result<(), HookRelayError> {
    if socket_path(config).as_os_str().as_encoded_bytes().len() > path_limit() {
        return Err(HookRelayError::EndpointTooLong);
    }
    Ok(())
}

/// What `bind` hands the accept loop.
#[cfg(unix)]
pub(crate) struct Bound {
    listener: tokio::net::UnixListener,
    path: std::path::PathBuf,
    /// The core's own user id: the owner of the socket file it just created.
    uid: u32,
}

#[cfg(unix)]
impl Bound {
    pub(crate) fn socket_path(&self) -> Option<std::path::PathBuf> {
        Some(self.path.clone())
    }
}

/// A.1: creates `<app data root>/run` with mode `0700` (an existing directory with wider
/// permissions is changed to `0700`), removes a stale `hook.sock` (a connect attempt fails with
/// `ECONNREFUSED`), refuses a socket another process accepts on (`HOOK_LISTEN_FAILED`), binds, then
/// sets mode `0600`.
#[cfg(unix)]
pub(crate) fn bind(config: &HookRelayConfig) -> Result<Bound, HookRelayError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let dir = socket_dir(config);
    let path = socket_path(config);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(HookRelayError::Listen)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(HookRelayError::Listen)?;
    if path.exists() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_) => {
                return Err(HookRelayError::Listen(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "another core accepts on the hook socket",
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                std::fs::remove_file(&path).map_err(HookRelayError::Listen)?;
            }
            Err(e) => return Err(HookRelayError::Listen(e)),
        }
    }
    let listener = tokio::net::UnixListener::bind(&path).map_err(HookRelayError::Listen)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(HookRelayError::Listen)?;
    let uid = std::fs::metadata(&path)
        .map_err(HookRelayError::Listen)?
        .uid();
    Ok(Bound {
        listener,
        path,
        uid,
    })
}

/// A.4: accepts until shutdown. At most 64 connections are served at once: the permit is taken
/// before `accept`, so the 65th waits in the accept backlog.
#[cfg(unix)]
pub(crate) async fn accept_loop(relay: Arc<HookRelay>, bound: Bound) {
    let Bound { listener, uid, .. } = bound;
    loop {
        let permit = tokio::select! {
            _ = relay.shutdown_signal.cancelled() => break,
            permit = relay.slots.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let accepted = tokio::select! {
            _ = relay.shutdown_signal.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let stream = match accepted {
            Ok((stream, _)) => stream,
            Err(error) => {
                hook_log!(
                    warn,
                    code = "HOOK_LISTEN_FAILED",
                    error_kind = format!("{:?}", error.kind()).as_str(),
                    "hook_accept_failed"
                );
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };
        // C.1: the peer's user id must be the core's.
        let allowed = stream
            .peer_cred()
            .map(|cred| cred.uid() == uid)
            .unwrap_or(false);
        let guard = ActiveGuard::new(&relay);
        let relay = relay.clone();
        tokio::spawn(async move {
            relay.serve(stream, allowed).await;
            drop(guard);
            drop(permit);
        });
    }
}

// ---- Windows

#[cfg(windows)]
fn pipe_name(config: &HookRelayConfig) -> String {
    format!(r"\\.\pipe\ordin-hook-{}", config.installation_id)
}

#[cfg(windows)]
pub(crate) fn endpoint_of(config: &HookRelayConfig) -> String {
    pipe_name(config)
}

#[cfg(windows)]
pub(crate) fn check_endpoint_length(_config: &HookRelayConfig) -> Result<(), HookRelayError> {
    Ok(())
}

#[cfg(windows)]
pub(crate) struct Bound {
    server: tokio::net::windows::named_pipe::NamedPipeServer,
    name: String,
}

#[cfg(windows)]
impl Bound {
    pub(crate) fn socket_path(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// A.2: the first pipe instance refuses remote clients; the pipe takes the default security
/// descriptor of the person's process (every call is also authenticated by its HMAC).
#[cfg(windows)]
pub(crate) fn bind(config: &HookRelayConfig) -> Result<Bound, HookRelayError> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = pipe_name(config);
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)
        .map_err(HookRelayError::Listen)?;
    Ok(Bound { server, name })
}

/// A.2, A.4: one new pipe instance is created after each accepted connection.
#[cfg(windows)]
pub(crate) async fn accept_loop(relay: Arc<HookRelay>, bound: Bound) {
    use tokio::net::windows::named_pipe::ServerOptions;
    let Bound { mut server, name } = bound;
    loop {
        let permit = tokio::select! {
            _ = relay.shutdown_signal.cancelled() => break,
            permit = relay.slots.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let connected = tokio::select! {
            _ = relay.shutdown_signal.cancelled() => break,
            connected = server.connect() => connected,
        };
        if connected.is_err() {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        }
        let next = match ServerOptions::new()
            .reject_remote_clients(true)
            .create(&name)
        {
            Ok(next) => next,
            Err(_) => break,
        };
        let stream = std::mem::replace(&mut server, next);
        let guard = ActiveGuard::new(&relay);
        let relay = relay.clone();
        tokio::spawn(async move {
            relay.serve(stream, true).await;
            drop(guard);
            drop(permit);
        });
    }
}

// ---- Other platforms

#[cfg(not(any(unix, windows)))]
pub(crate) fn endpoint_of(config: &HookRelayConfig) -> String {
    config
        .app_data_root
        .join("run")
        .join("hook.sock")
        .to_string_lossy()
        .into_owned()
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn check_endpoint_length(_config: &HookRelayConfig) -> Result<(), HookRelayError> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(crate) struct Bound;

#[cfg(not(any(unix, windows)))]
impl Bound {
    pub(crate) fn socket_path(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn bind(_config: &HookRelayConfig) -> Result<Bound, HookRelayError> {
    Err(HookRelayError::Listen(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "no local channel on this platform",
    )))
}

#[cfg(not(any(unix, windows)))]
pub(crate) async fn accept_loop(_relay: Arc<HookRelay>, _bound: Bound) {}
