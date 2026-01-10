//! What the program hands this crate at start (Public Interface *Construction*), and the server's
//! secret back-end rule (Internal Logic B; Configuration.md, the part that lands here).

use crate::error::{code, fail, CredError};

/// Which ordin program runs this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Desktop,
    PersonalServer,
    AgentSingleShape,
    AgentScaledShape,
}

/// Everything `IdentityService::new` needs. The crate calls no `std::env` function: the program
/// reads the environment and the profile, and passes the values here.
pub struct IdentityConfig {
    pub program: Program,
    /// Application data root (desktop: the platform app-data folder; server: ORDIN_DATA_DIR).
    pub data_root: std::path::PathBuf,
    /// Keychain service name: the branded build's bundle identifier (ADR-041), e.g. "io.gatblau.ordin".
    pub keychain_service: String,
    pub secret_backend: SecretBackendConfig,
    /// Compiled-in GitHub App client id (`option_env!("GITHUB_APP_CLIENT_ID")`); None disables GitHub.
    pub github_app_client_id: Option<String>,
    /// Shared client built by the program (rustls + platform verifier, system proxy; SA-HTTP-02).
    pub http: reqwest::Client,
    /// Opens a URL in the system browser. Implemented by the desktop program over
    /// `tauri-plugin-opener`; None on the server, which has no system browser.
    pub browser: Option<std::sync::Arc<dyn BrowserOpener>>,
}

/// Where secrets live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretBackendConfig {
    /// macOS login keychain, Windows Credential Manager (desktop).
    OsKeychain,
    /// The user session's Secret Service over D-Bus (server with a session bus).
    SecretService,
    /// `0600` file under the data root, key from a container secret file or a systemd credential.
    EncryptedFile { key_source: KeySource },
    /// Agent profile, scaled shape: no secret store (secrets come from ORDIN_SECRETS_DIR files read elsewhere).
    None,
}

/// Where the encrypted file's 32-byte key comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    KeyFile(std::path::PathBuf),
    SystemdCredential(std::path::PathBuf),
}

/// Pure rule that turns the server's environment values into a back end. The server program reads
/// `DBUS_SESSION_BUS_ADDRESS`, `ORDIN_SECRETS_KEY_FILE` and `ORDIN_SECRETS_KEY_CREDENTIAL` once at start
/// and passes their values here; the desktop program passes `SecretBackendConfig::OsKeychain` directly.
// spec: ordin-desktop/components/IdentityAndCredentials#SA-RECON-05
pub fn server_secret_backend(
    dbus_session_bus_address: Option<&str>,
    secrets_key_file: Option<&std::path::Path>,
    secrets_key_credential: Option<&std::path::Path>,
) -> Result<SecretBackendConfig, CredError> {
    match (secrets_key_file, secrets_key_credential) {
        // B.1: setting both stops the start with a named error.
        (Some(_), Some(_)) => fail(code::SECRET_KEY_CONFLICT),
        // B.2 and B.3.
        (Some(file), None) => Ok(SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(file.to_path_buf()),
        }),
        (None, Some(credential)) => Ok(SecretBackendConfig::EncryptedFile {
            key_source: KeySource::SystemdCredential(credential.to_path_buf()),
        }),
        // B.4 and B.5.
        (None, None) => match dbus_session_bus_address {
            Some(address) if !address.is_empty() => Ok(SecretBackendConfig::SecretService),
            _ => fail(code::SECRET_KEY_MISSING),
        },
    }
}

/// Implemented by the desktop program; the server program does not implement it.
pub trait BrowserOpener: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
}

/// A.1: `program` and `secret_backend` agree, and the pool is present unless the scaled shape runs.
pub(crate) fn check_config(config: &IdentityConfig, has_pool: bool) -> Result<(), CredError> {
    let scaled = config.program == Program::AgentScaledShape;
    let agrees = match config.secret_backend {
        SecretBackendConfig::OsKeychain => config.program == Program::Desktop,
        SecretBackendConfig::None => scaled,
        SecretBackendConfig::SecretService | SecretBackendConfig::EncryptedFile { .. } => true,
    };
    if !agrees || (!has_pool && !scaled) {
        return fail(code::CONFIG_INVALID);
    }
    Ok(())
}
