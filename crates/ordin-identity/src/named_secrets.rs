//! The narrow interfaces over fixed secret names (H.10), the client certificate (H.5, H.12) and the
//! systemd-encrypted secrets key (H.9).

use std::sync::Arc;

use rustls::sign::CertifiedKey;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::certificate::{self, StoredCertificate};
use crate::config::Program;
use crate::error::{code, fail, CredError};
use crate::secret::{random_fill, SecretName, MAX_SECRET_BYTES};
use crate::secret_file::{run_systemd_creds, SYSTEMD_CREDS_TIMEOUT};
use crate::service::{lock, short_hash, IdentityService};
use crate::types::{ClientCertificateView, DesktopKeyStore, ServerSecrets};

/// The Remote API's TLS private key (Fixed names, chosen in the spec).
const REMOTE_API_TLS_KEY: &str = "remote-api/tls-key";

/// `client-certificate/<hex SHA-256 of name>` after the 1–255 character check.
// spec: ordin-desktop/components/IdentityAndCredentials#SA-CLOSE-02
fn certificate_name(name: &str) -> Result<SecretName, CredError> {
    let chars = name.chars().count();
    if chars == 0 || chars > 255 {
        return fail(code::SECRET_NAME_INVALID);
    }
    let digest = Sha256::digest(name.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    SecretName::new(&format!("client-certificate/{hex}"))
}

impl IdentityService {
    /// Server only: makes a new 32-byte random key, encrypts it with `systemd-creds encrypt --user`
    /// into `credential_path` (mode 0600). Refuses an existing file (CRED_SECRET_KEY_EXISTS).
    pub async fn create_secrets_key_credential(
        credential_path: &std::path::Path,
    ) -> Result<(), CredError> {
        if tokio::fs::symlink_metadata(credential_path).await.is_ok() {
            return fail(code::SECRET_KEY_EXISTS);
        }
        let mut key = Zeroizing::new([0u8; 32]);
        random_fill(key.as_mut())?;
        let args = [
            std::ffi::OsStr::new("encrypt"),
            std::ffi::OsStr::new("--user"),
            std::ffi::OsStr::new("--name=ordin-secrets-key"),
            std::ffi::OsStr::new("-"),
            credential_path.as_os_str(),
        ];
        run_systemd_creds("systemd-creds", &args, key.as_ref(), SYSTEMD_CREDS_TIMEOUT).await?;
        set_owner_only(credential_path).await
    }

    /// The server setup command's `secret NAME` step: stores `value` (UTF-8) under `name`.
    pub async fn put_server_secret(&self, name: &str, value: &str) -> Result<(), CredError> {
        let name = SecretName::new(name)?;
        if value.is_empty() {
            return fail(code::PASSWORD_INVALID);
        }
        if value.len() > MAX_SECRET_BYTES {
            return fail(code::SECRET_TOO_LARGE);
        }
        self.inner.secrets.put(&name, value.as_bytes()).await
    }

    /// The client certificate the person imported under `name`; `Ok(None)` when none was imported.
    pub async fn client_certificate(
        &self,
        name: &str,
    ) -> Result<Option<Arc<CertifiedKey>>, CredError> {
        let secret_name = certificate_name(name)?;
        if self.inner.program != Program::Desktop {
            return fail(code::FLOW_NOT_OFFERED);
        }
        if let Some(cached) = lock(&self.inner.certificates).get(name) {
            return Ok(Some(Arc::clone(cached)));
        }
        let Some(bytes) = self.inner.secrets.get(&secret_name).await? else {
            return Ok(None);
        };
        let stored: StoredCertificate =
            serde_json::from_slice(&bytes).or_else(|_| fail(code::SECRET_STORE_CORRUPT))?;
        if stored.format != 1 {
            return fail(code::SECRET_STORE_CORRUPT);
        }
        let parsed = certificate::parse(
            stored.certificate_pem.as_bytes(),
            stored.key_pem.as_bytes(),
            code::SECRET_STORE_CORRUPT,
        )?;
        lock(&self.inner.certificates).insert(name.to_owned(), Arc::clone(&parsed.key));
        Ok(Some(parsed.key))
    }

    /// Imports a client certificate once (Settings › Sign-ins): PEM certificate chain and PEM
    /// private key, kept in the secret store under `name`. Replaces an earlier import.
    pub async fn import_client_certificate(
        &self,
        name: &str,
        certificate_pem: &[u8],
        key_pem: Zeroizing<Vec<u8>>,
    ) -> Result<ClientCertificateView, CredError> {
        if self.inner.program != Program::Desktop {
            return fail(code::FLOW_NOT_OFFERED);
        }
        let secret_name = certificate_name(name)?;
        let parsed = certificate::parse(certificate_pem, &key_pem, code::CERTIFICATE_INVALID)?;
        certificate::check_not_expired(&parsed)?;
        let stored = StoredCertificate {
            format: 1,
            certificate_pem: String::from_utf8(certificate_pem.to_vec())
                .or_else(|_| fail(code::CERTIFICATE_INVALID))?,
            key_pem: String::from_utf8(key_pem.to_vec())
                .or_else(|_| fail(code::CERTIFICATE_INVALID))?,
        };
        let bytes =
            Zeroizing::new(serde_json::to_vec(&stored).or_else(|_| fail(code::STORE_FAILED))?);
        self.inner.secrets.put(&secret_name, &bytes).await?;
        lock(&self.inner.certificates).clear();
        tracing::info!(
            target: "ordin_identity",
            name_hash = short_hash(name).as_str(),
            not_after = parsed.not_after.to_wire().as_str(),
            "cred.client_certificate.imported"
        );
        Ok(ClientCertificateView {
            name: name.to_owned(),
            subject: parsed.subject,
            not_after: parsed.not_after,
        })
    }
}

#[cfg(unix)]
async fn set_owner_only(path: &std::path::Path) -> Result<(), CredError> {
    use std::os::unix::fs::PermissionsExt as _;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .await
        .or_else(|_| fail(code::SYSTEMD_CREDS_FAILED))
}

#[cfg(not(unix))]
async fn set_owner_only(_path: &std::path::Path) -> Result<(), CredError> {
    Ok(())
}

#[async_trait::async_trait]
impl ServerSecrets for IdentityService {
    async fn remote_api_tls_key(&self) -> Result<Option<Zeroizing<String>>, CredError> {
        let name = SecretName::new(REMOTE_API_TLS_KEY)?;
        match self.inner.secrets.get(&name).await? {
            None => Ok(None),
            Some(bytes) => String::from_utf8(bytes.to_vec())
                .map(|pem| Some(Zeroizing::new(pem)))
                .or_else(|_| fail(code::SECRET_STORE_CORRUPT)),
        }
    }

    async fn put_remote_api_tls_key(&self, pem: Zeroizing<String>) -> Result<(), CredError> {
        if !pem.lines().any(|line| line.starts_with("-----BEGIN")) {
            return fail(code::PASSWORD_INVALID);
        }
        let name = SecretName::new(REMOTE_API_TLS_KEY)?;
        self.inner.secrets.put(&name, pem.as_bytes()).await
    }

    async fn delete_remote_api_tls_key(&self) -> Result<(), CredError> {
        let name = SecretName::new(REMOTE_API_TLS_KEY)?;
        self.inner.secrets.delete(&name).await
    }
}

#[async_trait::async_trait]
impl DesktopKeyStore for IdentityService {
    async fn put_desktop_key(&self, keychain_ref: &str, key: &[u8; 32]) -> Result<(), CredError> {
        let name = SecretName::new(keychain_ref)?;
        self.inner.secrets.put(&name, key).await
    }

    async fn get_desktop_key(
        &self,
        keychain_ref: &str,
    ) -> Result<Option<Zeroizing<[u8; 32]>>, CredError> {
        let name = SecretName::new(keychain_ref)?;
        let Some(bytes) = self.inner.secrets.get(&name).await? else {
            return Ok(None);
        };
        let key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .or_else(|_| fail(code::SECRET_STORE_CORRUPT))?;
        Ok(Some(Zeroizing::new(key)))
    }

    async fn delete_desktop_key(&self, keychain_ref: &str) -> Result<(), CredError> {
        let name = SecretName::new(keychain_ref)?;
        self.inner.secrets.delete(&name).await
    }

    async fn client_certificate(&self, name: &str) -> Result<Option<Arc<CertifiedKey>>, CredError> {
        IdentityService::client_certificate(self, name).await
    }
}
