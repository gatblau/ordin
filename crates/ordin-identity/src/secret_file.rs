//! The encrypted secrets file of a server with no Secret Service (Internal Logic D.3; C-50).
//!
//! `<data_root>/secrets/secrets.enc`, directory `0700`, file `0600`, JSON
//! `{"format":1,"nonce":"<base64, 24 bytes>","ciphertext":"<base64>"}` whose plaintext is a JSON
//! object name → base64 value, sealed with XChaCha20-Poly1305 and the associated data `ordin-secrets-v1`.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::config::KeySource;
use crate::error::{code, fail, CredError};
use crate::secret::random_fill;

const ASSOCIATED_DATA: &[u8] = b"ordin-secrets-v1";
const FORMAT: u32 = 1;
const NONCE_BYTES: usize = 24;
const KEY_BYTES: usize = 32;
/// `systemd-creds` timeout (Fixed constants).
pub(crate) const SYSTEMD_CREDS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize)]
struct Sealed {
    format: u32,
    nonce: String,
    ciphertext: String,
}

type Plain = BTreeMap<String, Zeroizing<Vec<u8>>>;

pub(crate) struct EncryptedFile {
    dir: PathBuf,
    key: Zeroizing<[u8; KEY_BYTES]>,
    /// The decrypted map; every `put`/`delete` holds this lock while it rewrites the file (D.3.4).
    map: tokio::sync::Mutex<Plain>,
}

impl EncryptedFile {
    /// D.3.1–D.3.3: read the key, make the folder, fix the file's mode, decrypt the file.
    pub(crate) async fn open(data_root: &Path, key_source: &KeySource) -> Result<Self, CredError> {
        let raw = match key_source {
            KeySource::KeyFile(path) => Zeroizing::new(
                tokio::fs::read(path)
                    .await
                    .or_else(|_| fail(code::SECRET_KEY_INVALID))?,
            ),
            KeySource::SystemdCredential(path) => systemd_decrypt(path).await?,
        };
        let key = parse_key(&raw)?;
        let dir = data_root.join("secrets");
        let opened = tokio::task::spawn_blocking(move || open_blocking(dir, key))
            .await
            .unwrap_or_else(|_| fail(code::SECRET_STORE_CORRUPT))?;
        Ok(opened)
    }

    pub(crate) async fn get(&self, name: &str) -> Option<Zeroizing<Vec<u8>>> {
        self.map.lock().await.get(name).cloned()
    }

    pub(crate) async fn put(&self, name: &str, value: &[u8]) -> Result<(), CredError> {
        let mut map = self.map.lock().await;
        let mut next = map.clone();
        next.insert(name.to_owned(), Zeroizing::new(value.to_vec()));
        self.write(&next).await?;
        *map = next;
        Ok(())
    }

    pub(crate) async fn delete(&self, name: &str) -> Result<(), CredError> {
        let mut map = self.map.lock().await;
        if !map.contains_key(name) {
            return Ok(());
        }
        let mut next = map.clone();
        next.remove(name);
        self.write(&next).await?;
        *map = next;
        Ok(())
    }

    /// D.3.4: the whole file, a fresh nonce, `secrets.enc.tmp` (0600), fsync, rename, fsync the folder.
    /// On failure the previous file stays intact and the map is unchanged.
    async fn write(&self, map: &Plain) -> Result<(), CredError> {
        let sealed = seal(&self.key, map)?;
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || write_blocking(&dir, &sealed))
            .await
            .unwrap_or_else(|_| fail(code::SECRET_STORE_WRITE_FAILED))
    }
}

/// D.3.1: exactly 32 raw bytes, or the standard base64 of 32 bytes, after trimming ASCII whitespace.
///
/// SPEC-GAP-2: a raw key is random bytes and may itself begin or end with a byte that is ASCII
/// whitespace; trimming such a key would refuse a key `create_secrets_key_credential` made. Input of
/// exactly 32 bytes is therefore taken as the raw key before trimming; every other input is trimmed
/// and read as the rule states.
pub(crate) fn parse_key(raw: &[u8]) -> Result<Zeroizing<[u8; KEY_BYTES]>, CredError> {
    let mut key = Zeroizing::new([0u8; KEY_BYTES]);
    if raw.len() == KEY_BYTES {
        key.copy_from_slice(raw);
        return Ok(key);
    }
    let trimmed = raw.trim_ascii();
    if trimmed.len() == KEY_BYTES {
        key.copy_from_slice(trimmed);
        return Ok(key);
    }
    let decoded = Zeroizing::new(
        STANDARD
            .decode(trimmed)
            .or_else(|_| fail(code::SECRET_KEY_INVALID))?,
    );
    if decoded.len() != KEY_BYTES {
        return fail(code::SECRET_KEY_INVALID);
    }
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn open_blocking(
    dir: PathBuf,
    key: Zeroizing<[u8; KEY_BYTES]>,
) -> Result<EncryptedFile, CredError> {
    if !dir.exists() {
        create_private_dir(&dir).or_else(|_| fail(code::SECRET_STORE_WRITE_FAILED))?;
    }
    let file = dir.join("secrets.enc");
    let map = match std::fs::read(&file) {
        Ok(bytes) => {
            fix_mode(&file);
            unseal(&key, &bytes)?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Plain::new(),
        Err(_) => return fail(code::SECRET_STORE_CORRUPT),
    };
    Ok(EncryptedFile {
        dir,
        key,
        map: tokio::sync::Mutex::new(map),
    })
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// D.3.2: a `secrets.enc` with a mode other than `0600` is set to `0600` (the systemd lab made `0644`).
#[cfg(unix)]
fn fix_mode(file: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let Ok(meta) = std::fs::metadata(file) else {
        return;
    };
    if meta.permissions().mode() & 0o777 != 0o600
        && std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600)).is_ok()
    {
        tracing::warn!(target: "ordin_identity", "cred.secret_file_mode_fixed");
    }
}

#[cfg(not(unix))]
fn fix_mode(_file: &Path) {}

fn cipher(key: &[u8; KEY_BYTES]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key.into())
}

fn seal(key: &[u8; KEY_BYTES], map: &Plain) -> Result<Zeroizing<Vec<u8>>, CredError> {
    let mut encoded: BTreeMap<&str, String> = map
        .iter()
        .map(|(name, value)| (name.as_str(), STANDARD.encode(value)))
        .collect();
    let plain = serde_json::to_vec(&encoded).map(Zeroizing::new);
    encoded.values_mut().for_each(Zeroize::zeroize);
    let plain = plain.or_else(|_| fail(code::SECRET_STORE_WRITE_FAILED))?;
    let mut nonce = [0u8; NONCE_BYTES];
    random_fill(&mut nonce).or_else(|_| fail(code::SECRET_STORE_WRITE_FAILED))?;
    let ciphertext = cipher(key)
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: ASSOCIATED_DATA,
            },
        )
        .or_else(|_| fail(code::SECRET_STORE_WRITE_FAILED))?;
    let sealed = Sealed {
        format: FORMAT,
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    };
    serde_json::to_vec(&sealed)
        .map(Zeroizing::new)
        .or_else(|_| fail(code::SECRET_STORE_WRITE_FAILED))
}

/// D.3.3: a wrong key, a tampered file or a format other than 1 is `CRED_SECRET_STORE_CORRUPT`.
fn unseal(key: &[u8; KEY_BYTES], bytes: &[u8]) -> Result<Plain, CredError> {
    let corrupt = || CredError::of(code::SECRET_STORE_CORRUPT);
    let sealed: Sealed = serde_json::from_slice(bytes).map_err(|_| corrupt())?;
    if sealed.format != FORMAT {
        return Err(corrupt());
    }
    let nonce = STANDARD.decode(&sealed.nonce).map_err(|_| corrupt())?;
    if nonce.len() != NONCE_BYTES {
        return Err(corrupt());
    }
    let ciphertext = STANDARD.decode(&sealed.ciphertext).map_err(|_| corrupt())?;
    let plain = Zeroizing::new(
        cipher(key)
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: ASSOCIATED_DATA,
                },
            )
            .map_err(|_| corrupt())?,
    );
    let mut encoded: BTreeMap<String, String> =
        serde_json::from_slice(&plain).map_err(|_| corrupt())?;
    let decoded = encoded
        .iter()
        .map(|(name, value)| {
            let bytes = STANDARD.decode(value.as_bytes()).map_err(|_| corrupt())?;
            Ok((name.clone(), Zeroizing::new(bytes)))
        })
        .collect();
    encoded.values_mut().for_each(Zeroize::zeroize);
    decoded
}

fn write_blocking(dir: &Path, sealed: &[u8]) -> Result<(), CredError> {
    let failed = |_| CredError::of(code::SECRET_STORE_WRITE_FAILED);
    let tmp = dir.join("secrets.enc.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp).map_err(failed)?;
    file.write_all(sealed).map_err(failed)?;
    file.sync_all().map_err(failed)?;
    drop(file);
    std::fs::rename(&tmp, dir.join("secrets.enc")).map_err(failed)?;
    #[cfg(unix)]
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(failed)?;
    Ok(())
}

/// The environment `systemd-creds` runs with: cleared except `PATH` and `XDG_RUNTIME_DIR` (D.3.1).
fn systemd_command(program: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command.env_clear();
    for name in ["PATH", "XDG_RUNTIME_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    command
}

/// Runs `program args…` with `input` on standard input; returns standard output. A missing binary,
/// a non-zero exit or the 10-second timeout is `CRED_SYSTEMD_CREDS_FAILED` (D.3.1, H.9). Only the exit
/// status is logged, never the output.
pub(crate) async fn run_systemd_creds(
    program: &str,
    args: &[&std::ffi::OsStr],
    input: &[u8],
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, CredError> {
    use tokio::io::AsyncWriteExt as _;
    let failed = || CredError::of(code::SYSTEMD_CREDS_FAILED);
    let mut command = systemd_command(program);
    command.args(args);
    let run = async {
        let mut child = command.spawn().map_err(|_| failed())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input).await.map_err(|_| failed())?;
            drop(stdin);
        }
        let output = child.wait_with_output().await.map_err(|_| failed())?;
        if !output.status.success() {
            // SPEC-GAP-5: the Error Table says the exit status is logged but names no event; this
            // event name is chosen here.
            tracing::warn!(
                target: "ordin_identity",
                code = code::SYSTEMD_CREDS_FAILED,
                exit_status = output.status.code().unwrap_or(-1),
                "cred.systemd_creds.failed"
            );
            return Err(failed());
        }
        Ok(Zeroizing::new(output.stdout))
    };
    tokio::time::timeout(timeout, run)
        .await
        .unwrap_or_else(|_| Err(failed()))
}

/// `systemd-creds decrypt --user <p> -`.
async fn systemd_decrypt(path: &Path) -> Result<Zeroizing<Vec<u8>>, CredError> {
    let args = [
        std::ffi::OsStr::new("decrypt"),
        std::ffi::OsStr::new("--user"),
        path.as_os_str(),
        std::ffi::OsStr::new("-"),
    ];
    run_systemd_creds("systemd-creds", &args, &[], SYSTEMD_CREDS_TIMEOUT).await
}
