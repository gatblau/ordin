//! The secret store every module uses for named secrets (Public Interface *Secret store*;
//! Internal Logic D, H.7, H.8).

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use zeroize::Zeroizing;

use crate::error::{code, fail, CredError};
use crate::secret_file::EncryptedFile;

/// The largest value the store keeps: 64 KiB.
pub(crate) const MAX_SECRET_BYTES: usize = 64 * 1024;
/// The longest secret name: the pattern's 1 + 199 characters.
const MAX_NAME_CHARS: usize = 200;

/// A secret that never prints: Debug and Display write "[redacted]"; zeroised on drop.
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    pub fn new(s: String) -> Self {
        SecretString(Zeroizing::new(s))
    }
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Validated name, pattern `^[a-z0-9][a-z0-9_./-]{0,199}$`. Convention: `<module>/<purpose>/<id>`,
/// e.g. `server-connector/desktop-key/0192…`, `telegram/bot-token`, `credential/github/0192…`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretName(String);

impl SecretName {
    pub fn new(s: &str) -> Result<Self, CredError> {
        let bytes = s.as_bytes();
        let first_ok = bytes
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        let rest_ok = bytes.iter().skip(1).all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-')
        });
        if !first_ok || !rest_ok || bytes.len() > MAX_NAME_CHARS {
            return fail(code::SECRET_NAME_INVALID);
        }
        Ok(SecretName(s.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which back end a store uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretBackendKind {
    OsKeychain,
    SecretService,
    EncryptedFile,
    None,
}

impl SecretBackendKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SecretBackendKind::OsKeychain => "os_keychain",
            SecretBackendKind::SecretService => "secret_service",
            SecretBackendKind::EncryptedFile => "encrypted_file",
            SecretBackendKind::None => "none",
        }
    }
}

/// The platform store behind `OsKeychain` and `SecretService`: blocking calls, run on
/// `spawn_blocking`. One implementation over `keyring`; the unit tests add an in-memory one.
pub(crate) trait Keychain: Send + Sync + 'static {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, CredError>;
    fn set(&self, name: &str, value: &[u8]) -> Result<(), CredError>;
    fn delete(&self, name: &str) -> Result<(), CredError>;
}

/// D.1 and D.2: `keyring` 3, one entry per name under the build's keychain service.
pub(crate) struct KeyringKeychain {
    pub(crate) service: String,
}

/// `NoEntry` is handled by the caller; every other `keyring` error is a locked, refused or
/// unreachable store (D.1: `PlatformFailure`, `NoStorageAccess`; D.2: a D-Bus error).
fn keyring_failure(_: keyring::Error) -> CredError {
    CredError::of(code::KEYCHAIN_UNAVAILABLE)
}

impl Keychain for KeyringKeychain {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>, CredError> {
        let entry = keyring::Entry::new(&self.service, name).map_err(keyring_failure)?;
        match entry.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(other) => Err(keyring_failure(other)),
        }
    }

    fn set(&self, name: &str, value: &[u8]) -> Result<(), CredError> {
        let entry = keyring::Entry::new(&self.service, name).map_err(keyring_failure)?;
        entry.set_secret(value).map_err(keyring_failure)
    }

    fn delete(&self, name: &str) -> Result<(), CredError> {
        let entry = keyring::Entry::new(&self.service, name).map_err(keyring_failure)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(other) => Err(keyring_failure(other)),
        }
    }
}

pub(crate) enum Backend {
    Keychain {
        kind: SecretBackendKind,
        keychain: Arc<dyn Keychain>,
    },
    File(EncryptedFile),
    None,
}

/// The one secret store of a running program.
pub struct SecretStore {
    backend: Backend,
    /// One writer at a time per name for `get_or_create_random` (H.7).
    creating: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl SecretStore {
    pub(crate) fn from_backend(backend: Backend) -> Self {
        SecretStore {
            backend,
            creating: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn put(&self, name: &SecretName, value: &[u8]) -> Result<(), CredError> {
        if value.len() > MAX_SECRET_BYTES {
            return fail(code::SECRET_TOO_LARGE);
        }
        match &self.backend {
            Backend::Keychain { keychain, .. } => {
                let keychain = Arc::clone(keychain);
                let name = name.as_str().to_owned();
                let value = Zeroizing::new(value.to_vec());
                blocking(move || keychain.set(&name, &value)).await
            }
            Backend::File(file) => file.put(name.as_str(), value).await,
            Backend::None => fail(code::SECRET_STORE_ABSENT),
        }
    }

    /// Ok(None) when absent.
    pub async fn get(&self, name: &SecretName) -> Result<Option<Zeroizing<Vec<u8>>>, CredError> {
        match &self.backend {
            Backend::Keychain { keychain, .. } => {
                let keychain = Arc::clone(keychain);
                let name = name.as_str().to_owned();
                let value = blocking(move || keychain.get(&name)).await?;
                Ok(value.map(Zeroizing::new))
            }
            Backend::File(file) => Ok(file.get(name.as_str()).await),
            Backend::None => fail(code::SECRET_STORE_ABSENT),
        }
    }

    pub async fn delete(&self, name: &SecretName) -> Result<(), CredError> {
        match &self.backend {
            Backend::Keychain { keychain, .. } => {
                let keychain = Arc::clone(keychain);
                let name = name.as_str().to_owned();
                blocking(move || keychain.delete(&name)).await
            }
            Backend::File(file) => file.delete(name.as_str()).await,
            Backend::None => fail(code::SECRET_STORE_ABSENT),
        }
    }

    pub fn backend(&self) -> SecretBackendKind {
        match &self.backend {
            Backend::Keychain { kind, .. } => *kind,
            Backend::File(_) => SecretBackendKind::EncryptedFile,
            Backend::None => SecretBackendKind::None,
        }
    }

    /// The value under `name`, or `len` (16–64) random bytes written there first when absent
    /// (an HMAC key a module needs once per installation). One writer at a time per name.
    pub async fn get_or_create_random(
        &self,
        name: &SecretName,
        len: usize,
    ) -> Result<Zeroizing<Vec<u8>>, CredError> {
        if !(16..=64).contains(&len) {
            return fail(code::SECRET_TOO_LARGE);
        }
        let lock = {
            let mut creating = self
                .creating
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(creating.entry(name.as_str().to_owned()).or_default())
        };
        let _one_writer = lock.lock().await;
        if let Some(value) = self.get(name).await? {
            return Ok(value);
        }
        let mut value = Zeroizing::new(vec![0u8; len]);
        random_fill(&mut value)?;
        self.put(name, &value).await?;
        Ok(value)
    }

    /// The key the person entered for `key_name` of an agent instance (SA-CRED-01), stored under
    /// `agent-mcp/<agent instance id>/<key_name>`. `Ok(None)` when not entered.
    pub async fn agent_key(
        &self,
        agent_instance_id: ordin_types::AgentInstanceId,
        key_name: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CredError> {
        let name = SecretName::new(&format!("agent-mcp/{agent_instance_id}/{key_name}"))?;
        self.get(&name).await
    }
}

/// Runs a blocking keychain call off the runtime's worker threads.
async fn blocking<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, CredError> + Send + 'static,
) -> Result<T, CredError> {
    tokio::task::spawn_blocking(call)
        .await
        .unwrap_or_else(|_| fail(code::KEYCHAIN_UNAVAILABLE))
}

/// Fills `buf` from the operating system's random source.
pub(crate) fn random_fill(buf: &mut [u8]) -> Result<(), CredError> {
    // A platform without a random source cannot keep secrets at all; the store is unavailable.
    getrandom::fill(buf).or_else(|_| fail(code::KEYCHAIN_UNAVAILABLE))
}
