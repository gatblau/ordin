//! The secret-store scenarios of the Acceptance Criteria (X2) and the encrypted file's own rules.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;

use super::harness::{code_of, key_file, migrated_pool, temp_dir};
use crate::config::{IdentityConfig, KeySource, Program, SecretBackendConfig};
use crate::secret::SecretName;
use crate::secret_file::{parse_key, run_systemd_creds};
use crate::service::IdentityService;

fn server(dir: &std::path::Path, key_source: KeySource) -> IdentityConfig {
    IdentityConfig {
        program: Program::PersonalServer,
        data_root: dir.to_path_buf(),
        keychain_service: "io.gatblau.ordin.test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile { key_source },
        github_app_client_id: None,
        microsoft_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    }
}

async fn open(dir: &std::path::Path, key: [u8; 32]) -> Result<IdentityService, crate::CredError> {
    let pool = migrated_pool(dir).await;
    let source = KeySource::KeyFile(key_file(dir, key));
    IdentityService::new(server(dir, source), Some(pool)).await
}

// Scenario: Happy path — encrypted secrets file on a server
#[tokio::test]
async fn happy_path_encrypted_secrets_file_on_a_server() {
    let dir = temp_dir();
    let key_path = key_file(&dir, [3u8; 32]);
    let backend = crate::server_secret_backend(None, Some(&key_path), None).unwrap();
    assert_eq!(
        backend,
        SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_path.clone())
        }
    );
    let name = SecretName::new("telegram/bot-token").unwrap();
    {
        let service = open(&dir, [3u8; 32])
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        service.secrets().put(&name, b"123:abc").await.unwrap();
    }
    let file = dir.join("secrets/secrets.enc");
    assert!(file.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(dir.join("secrets"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let raw = std::fs::read_to_string(&file).unwrap();
    assert!(!raw.contains("123:abc"));
    let restarted = open(&dir, [3u8; 32])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        restarted
            .secrets()
            .get(&name)
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        b"123:abc"
    );
}

// Scenario: Error — both key variables set
#[test]
fn error_both_key_variables_set() {
    let result = crate::server_secret_backend(
        Some("unix:path=/run/user/1000/bus"),
        Some(std::path::Path::new("/run/secrets/ordin-key")),
        Some(std::path::Path::new(
            "/home/ordin/.config/ordin/secrets.key.cred",
        )),
    );
    assert_eq!(code_of(result), "CRED_SECRET_KEY_CONFLICT");
}

#[test]
fn the_back_end_rule_picks_the_credential_then_the_session_bus() {
    let cred = std::path::Path::new("/home/ordin/.config/ordin/secrets.key.cred");
    assert_eq!(
        crate::server_secret_backend(None, None, Some(cred)).unwrap(),
        SecretBackendConfig::EncryptedFile {
            key_source: KeySource::SystemdCredential(cred.to_path_buf())
        }
    );
    assert_eq!(
        crate::server_secret_backend(Some("unix:path=/run/user/1000/bus"), None, None).unwrap(),
        SecretBackendConfig::SecretService
    );
}

// Scenario: Error — wrong key
#[tokio::test]
async fn error_wrong_key() {
    let dir = temp_dir();
    {
        let service = open(&dir, [1u8; 32])
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let name = SecretName::new("telegram/bot-token").unwrap();
        service.secrets().put(&name, b"value").await.unwrap();
    }
    let file = dir.join("secrets/secrets.enc");
    let before = std::fs::read(&file).unwrap();
    std::fs::remove_file(dir.join("journal.db")).ok();
    let result = open(&dir, [2u8; 32]).await;
    assert_eq!(code_of(result), "CRED_SECRET_STORE_CORRUPT");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "the file is unchanged"
    );
}

// Scenario: Error — systemd credential cannot be decrypted
#[tokio::test]
async fn error_systemd_credential_cannot_be_decrypted() {
    let dir = temp_dir();
    let credential = dir.join("secrets.key.cred");
    std::fs::write(&credential, b"Another user's credential").unwrap();
    let pool = migrated_pool(&dir).await;
    let result = IdentityService::new(
        server(&dir, KeySource::SystemdCredential(credential)),
        Some(pool),
    )
    .await;
    assert_eq!(code_of(result), "CRED_SYSTEMD_CREDS_FAILED");
}

#[test]
fn a_key_is_32_raw_bytes_or_the_base64_of_32_bytes() {
    // Raw bytes that begin and end with ASCII whitespace are still a raw key.
    let mut raw = [5u8; 32];
    raw[0] = b' ';
    raw[31] = b'\n';
    assert_eq!(parse_key(&raw).unwrap().as_slice(), &raw);
    let encoded = format!("{}\n", STANDARD.encode([6u8; 32]));
    assert_eq!(
        parse_key(encoded.as_bytes()).unwrap().as_slice(),
        &[6u8; 32]
    );
    let mut padded = b"  ".to_vec();
    padded.extend_from_slice(&[8u8; 32]);
    assert_eq!(parse_key(&padded).unwrap().as_slice(), &[8u8; 32]);
    assert_eq!(
        code_of(parse_key(STANDARD.encode([6u8; 31]).as_bytes())),
        "CRED_SECRET_KEY_INVALID"
    );
    assert_eq!(
        code_of(parse_key(b"not base64 at all!")),
        "CRED_SECRET_KEY_INVALID"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_secrets_file_with_a_wider_mode_is_set_to_0600() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir();
    {
        let service = open(&dir, [4u8; 32])
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let name = SecretName::new("nats/credentials").unwrap();
        service.secrets().put(&name, b"v").await.unwrap();
    }
    let file = dir.join("secrets/secrets.enc");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::remove_file(dir.join("journal.db")).ok();
    open(&dir, [4u8; 32])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[tokio::test]
async fn delete_removes_a_secret_and_succeeds_when_absent() {
    let dir = temp_dir();
    let service = open(&dir, [9u8; 32])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let name = SecretName::new("nats/credentials").unwrap();
    service.secrets().put(&name, b"v").await.unwrap();
    service.secrets().delete(&name).await.unwrap();
    service.secrets().delete(&name).await.unwrap();
    assert!(service.secrets().get(&name).await.unwrap().is_none());
    assert_eq!(
        service.secrets().backend(),
        crate::SecretBackendKind::EncryptedFile
    );
}

#[tokio::test]
async fn an_agent_key_is_read_under_its_agent_instance() {
    let dir = temp_dir();
    let service = open(&dir, [9u8; 32])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let agent = ordin_types::AgentInstanceId::new();
    let name = SecretName::new(&format!("agent-mcp/{agent}/tickets")).unwrap();
    service.secrets().put(&name, b"key").await.unwrap();
    assert_eq!(
        service
            .secrets()
            .agent_key(agent, "tickets")
            .await
            .unwrap()
            .unwrap()
            .as_slice(),
        b"key"
    );
    assert!(service
        .secrets()
        .agent_key(agent, "other")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        code_of(service.secrets().agent_key(agent, "Bad Name").await),
        "CRED_SECRET_NAME_INVALID"
    );
    assert_eq!(
        code_of(service.secrets().get_or_create_random(&name, 8).await),
        "CRED_SECRET_TOO_LARGE"
    );
}

#[tokio::test]
async fn put_server_secret_checks_its_value() {
    let dir = temp_dir();
    let service = open(&dir, [9u8; 32])
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    service
        .put_server_secret("telegram/bot-token", "123:abc")
        .await
        .unwrap();
    assert_eq!(
        code_of(service.put_server_secret("telegram/bot-token", "").await),
        "CRED_PASSWORD_INVALID"
    );
    assert_eq!(
        code_of(service.put_server_secret("Bad", "v").await),
        "CRED_SECRET_NAME_INVALID"
    );
    let big = "x".repeat(64 * 1024 + 1);
    assert_eq!(
        code_of(service.put_server_secret("telegram/bot-token", &big).await),
        "CRED_SECRET_TOO_LARGE"
    );
}

/// `systemd-creds` is run with no shell, a cleared environment, the key on standard input, and a
/// timeout; a stand-in program shows each outcome.
#[cfg(unix)]
#[tokio::test]
async fn the_systemd_creds_runner_passes_input_and_honours_the_timeout() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = temp_dir();
    let script = |name: &str, body: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    };
    let echo = script("echo-stdin", "cat");
    let out = run_systemd_creds(&echo, &[], b"key-bytes", Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(out.as_slice(), b"key-bytes");
    let failing = script("fail", "exit 3");
    assert_eq!(
        code_of(run_systemd_creds(&failing, &[], b"", Duration::from_secs(5)).await),
        "CRED_SYSTEMD_CREDS_FAILED"
    );
    let slow = script("slow", "sleep 5");
    assert_eq!(
        code_of(run_systemd_creds(&slow, &[], b"", Duration::from_millis(200)).await),
        "CRED_SYSTEMD_CREDS_FAILED"
    );
    let env = script("env", "env");
    let out = run_systemd_creds(&env, &[], b"", Duration::from_secs(5))
        .await
        .unwrap();
    let text = String::from_utf8(out.to_vec()).unwrap();
    assert!(text.lines().all(|l| l.starts_with("PATH=")
        || l.starts_with("XDG_RUNTIME_DIR=")
        || l.starts_with("PWD=")
        || l.starts_with("SHLVL=")
        || l.starts_with("_=")));
}

// ---- SA-CRED-VAULT-1 to SA-CRED-VAULT-3: one keychain entry unlocks every secret.

mod vault {
    use std::sync::atomic::Ordering;

    use ordin_types::CodedError as _;

    use super::super::harness::Harness;
    use crate::secret::{SecretName, VAULT_KEY_NAME};

    fn name(s: &str) -> SecretName {
        SecretName::new(s).unwrap()
    }

    fn keychain_names(h: &Harness) -> Vec<String> {
        let mut names: Vec<String> = h.keychain.items.lock().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    // spec: ordin-desktop/components/IdentityAndCredentials#SA-CRED-VAULT-1
    #[tokio::test]
    async fn happy_path_the_keychain_is_read_once() {
        let h = Harness::new().await;
        let store = h.service.secrets();
        for (n, v) in [("telegram/bot-token", "1"), ("obs/password", "2"), ("credential/x/1", "3")] {
            store.put(&name(n), v.as_bytes()).await.unwrap();
        }
        for (n, v) in [("telegram/bot-token", "1"), ("obs/password", "2"), ("credential/x/1", "3")] {
            assert_eq!(store.get(&name(n)).await.unwrap().unwrap().as_slice(), v.as_bytes());
        }
        assert_eq!(keychain_names(&h), [VAULT_KEY_NAME]);
        assert_eq!(h.keychain.gets.load(Ordering::SeqCst), 1, "one keychain read");
        let file = std::fs::read_to_string(h.dir.join("secrets/secrets.enc")).unwrap();
        assert!(!file.contains("telegram"), "names and values are sealed");
    }

    // spec: ordin-desktop/components/IdentityAndCredentials#SA-CRED-VAULT-2
    #[tokio::test]
    async fn edge_a_secret_an_earlier_build_left_moves_into_the_vault() {
        let h = Harness::new().await;
        h.keychain
            .items
            .lock()
            .unwrap()
            .insert("telegram/bot-token".to_owned(), b"123:abc".to_vec());
        let store = h.service.secrets();
        let value = store.get(&name("telegram/bot-token")).await.unwrap().unwrap();
        assert_eq!(value.as_slice(), b"123:abc");
        assert_eq!(keychain_names(&h), [VAULT_KEY_NAME], "the old entry is gone");
        assert_eq!(h.secret_names().await, ["telegram/bot-token"]);
        let gets = h.keychain.gets.load(Ordering::SeqCst);
        assert!(store.get(&name("telegram/bot-token")).await.unwrap().is_some());
        assert!(store.get(&name("absent/one")).await.unwrap().is_none());
        assert!(store.get(&name("absent/one")).await.unwrap().is_none());
        assert_eq!(
            h.keychain.gets.load(Ordering::SeqCst),
            gets + 1,
            "an absent name is looked up once; a moved one never again"
        );
    }

    // spec: ordin-desktop/components/IdentityAndCredentials#SA-CRED-VAULT-2
    #[tokio::test]
    async fn edge_a_deleted_secret_is_gone_from_its_old_entry_too() {
        let h = Harness::new().await;
        h.keychain
            .items
            .lock()
            .unwrap()
            .insert("telegram/bot-token".to_owned(), b"old".to_vec());
        h.service.secrets().delete(&name("telegram/bot-token")).await.unwrap();
        assert_eq!(keychain_names(&h), [VAULT_KEY_NAME]);
        assert!(h
            .service
            .secrets()
            .get(&name("telegram/bot-token"))
            .await
            .unwrap()
            .is_none());
    }

    // spec: ordin-desktop/components/IdentityAndCredentials#SA-CRED-VAULT-1
    #[tokio::test]
    async fn error_a_locked_keychain_at_start_is_unavailable_and_tried_again() {
        let h = Harness::new().await;
        h.lock_keychain().await;
        let result = h.service.secrets().get(&name("telegram/bot-token")).await;
        let error = result.expect_err("locked");
        assert_eq!(error.code(), "CRED_KEYCHAIN_UNAVAILABLE");
        assert!(error.retryable());
        h.keychain.locked.store(false, Ordering::SeqCst);
        assert!(h
            .service
            .secrets()
            .get(&name("telegram/bot-token"))
            .await
            .unwrap()
            .is_none());
    }

    // spec: ordin-desktop/components/IdentityAndCredentials#SA-CRED-VAULT-3
    #[tokio::test]
    async fn edge_a_file_without_its_key_is_set_aside() {
        let h = Harness::new().await;
        let store = h.service.secrets();
        store.put(&name("telegram/bot-token"), b"123").await.unwrap();
        h.keychain.items.lock().unwrap().remove(VAULT_KEY_NAME);
        store.restart().await;
        assert!(store.get(&name("telegram/bot-token")).await.unwrap().is_none());
        let aside: Vec<String> = std::fs::read_dir(h.dir.join("secrets"))
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("secrets.enc.unreadable-"))
            .collect();
        assert_eq!(aside.len(), 1, "the old file is kept, renamed");
        assert_eq!(keychain_names(&h), [VAULT_KEY_NAME], "a new key");
    }
}
