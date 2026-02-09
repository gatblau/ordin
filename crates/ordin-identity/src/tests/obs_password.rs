//! The OBS password (IdentityAndCredentials.md ID-OBS-1): the amendment's Gherkin, one test per
//! scenario named after it, and its Error Table rows.

use std::path::Path;
use std::sync::atomic::Ordering;

use super::harness::{code_of, Capture, Harness, Setup, Store};
use crate::config::Program;
use crate::secret::SecretString;

const PASSWORD: &str = "fixture-pass";

fn secret(text: &str) -> SecretString {
    SecretString::new(text.to_owned())
}

/// True when any file under `dir`, read as bytes, holds `needle`.
fn any_file_holds(dir: &Path, needle: &[u8]) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            any_file_holds(&path, needle)
        } else {
            std::fs::read(&path)
                .map(|bytes| bytes.windows(needle.len()).any(|w| w == needle))
                .unwrap_or(false)
        }
    })
}

// Scenario: Happy path — saved and read back
#[tokio::test]
async fn happy_path_saved_and_read_back() {
    let h = Harness::new().await;
    h.service.save_obs_password(secret(PASSWORD)).await.unwrap();
    let read = h.service.obs_password().await.unwrap().unwrap();
    assert_eq!(read.expose(), PASSWORD);
    assert_eq!(
        h.count("SELECT COUNT(*) FROM credentials").await,
        0,
        "no credential row"
    );
    assert!(h.service.list().await.unwrap().is_empty());
    // SA-CRED-VAULT-1: kept in the vault under its name.
    assert_eq!(h.secret_names().await, ["obs-websocket"]);
    // A later save replaces the earlier password.
    h.service
        .save_obs_password(secret("second-pass"))
        .await
        .unwrap();
    assert_eq!(
        h.service.obs_password().await.unwrap().unwrap().expose(),
        "second-pass"
    );
    assert_eq!(format!("{read:?}"), "[redacted]");
}

// Scenario: Edge — none saved
#[tokio::test]
async fn edge_none_saved() {
    let h = Harness::new().await;
    assert!(h.service.obs_password().await.unwrap().is_none());
}

// Scenario: Error — empty password
#[tokio::test]
async fn error_empty_password() {
    let h = Harness::new().await;
    assert_eq!(
        code_of(h.service.save_obs_password(secret("")).await),
        "CRED_PASSWORD_INVALID"
    );
    assert_eq!(h.keychain.puts.load(Ordering::SeqCst), 0);
}

// Scenario: Edge — the password appears nowhere else
#[tokio::test]
async fn edge_the_password_appears_nowhere_else() {
    let capture = Capture::default();
    let _guard = capture.install();
    let h = Harness::with(Setup {
        store: Store::File,
        ..Setup::default()
    })
    .await;
    h.service.save_obs_password(secret(PASSWORD)).await.unwrap();
    assert_eq!(
        h.service.obs_password().await.unwrap().unwrap().expose(),
        PASSWORD
    );
    h.pool.close().await;
    assert!(
        !any_file_holds(&h.dir, PASSWORD.as_bytes()),
        "a file under the data folder holds the password"
    );
    assert!(
        !capture.text().contains(PASSWORD),
        "a log line holds the password"
    );
}

// Error Table: password over 256 characters → CRED_PASSWORD_INVALID; 256 is kept.
#[tokio::test]
async fn a_password_over_256_characters_is_refused() {
    let h = Harness::new().await;
    assert_eq!(
        code_of(h.service.save_obs_password(secret(&"é".repeat(257))).await),
        "CRED_PASSWORD_INVALID"
    );
    h.service
        .save_obs_password(secret(&"é".repeat(256)))
        .await
        .unwrap();
}

// Error Table: `save_obs_password` on the server program → CRED_FLOW_NOT_OFFERED.
#[tokio::test]
async fn saving_on_the_server_program_is_not_offered() {
    let h = Harness::with(Setup {
        program: Program::PersonalServer,
        ..Setup::default()
    })
    .await;
    assert_eq!(
        code_of(h.service.save_obs_password(secret(PASSWORD)).await),
        "CRED_FLOW_NOT_OFFERED"
    );
}

// Error Table: secret store locked, refused or unreachable → CRED_KEYCHAIN_UNAVAILABLE.
#[tokio::test]
async fn a_locked_secret_store_is_keychain_unavailable() {
    let h = Harness::new().await;
    h.lock_keychain().await;
    assert_eq!(
        code_of(h.service.save_obs_password(secret(PASSWORD)).await),
        "CRED_KEYCHAIN_UNAVAILABLE"
    );
    assert_eq!(
        code_of(h.service.obs_password().await),
        "CRED_KEYCHAIN_UNAVAILABLE"
    );
}
