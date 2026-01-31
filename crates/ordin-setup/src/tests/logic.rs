//! Table-driven tests of the crate's pure rules (version token, archive paths, update addresses,
//! signatures), FU-094's `PATH` check and the built-in Claude Code probe against a
//! stand-in `claude` script.

use std::ffi::OsString;
use std::path::Path;

use crate::process::version_token;
#[cfg(unix)]
use crate::process::{find_on_path, ClaudeCodeProbe};
use crate::tools::missing_path_tools;
use crate::types::MissingTool;
#[cfg(unix)]
use crate::types::{HarnessProbe, HarnessSignIn};
use crate::updates::{https_or_loopback, safe_entry_path, verify_signature};

use super::{write_script, TempDir};

#[test]
fn version_token_takes_the_first_semantic_version() {
    let cases = [
        ("2.1.302 (Claude Code)", Some("2.1.302")),
        ("claude version 1.0.12-beta", Some("1.0.12-beta")),
        ("ordin-hook 0.1.0", Some("0.1.0")),
        ("v2.1.3", None),
        ("2.1", None),
        ("no version here", None),
        ("", None),
    ];
    for (output, expected) in cases {
        assert_eq!(version_token(output).as_deref(), expected, "{output:?}");
    }
}

#[test]
fn archive_entry_paths_must_stay_inside() {
    let cases = [
        ("bin/ordin-server", true),
        ("./bin/ordin-hook", true),
        ("rust-analyzer", true),
        ("../escape", false),
        ("bin/../../escape", false),
        ("/etc/passwd", false),
        ("", false),
    ];
    for (path, expected) in cases {
        assert_eq!(safe_entry_path(Path::new(path)), expected, "{path:?}");
    }
}

#[test]
fn update_addresses_are_https_or_test_loopback() {
    let cases = [
        ("https://updates.example/ordin/latest.json", true),
        ("http://127.0.0.1:8080/latest.json", true),
        ("http://localhost/latest.json", true),
        ("http://updates.example/latest.json", false),
        ("ftp://updates.example/latest.json", false),
        ("not a url", false),
    ];
    for (address, expected) in cases {
        assert_eq!(https_or_loopback(address).is_some(), expected, "{address}");
    }
}

#[test]
fn a_signature_without_a_key_or_in_bad_form_is_refused() {
    assert!(!verify_signature(None, b"bytes", "c2lnbmF0dXJl"));
    assert!(!verify_signature(
        Some("not base64!"),
        b"bytes",
        "c2lnbmF0dXJl"
    ));
    assert!(!verify_signature(Some("a2V5"), b"bytes", "c2lnbmF0dXJl"));
}

/// A folder holding an executable `name` for each of `names` (`name.exe` on Windows).
fn path_with(names: &[&str]) -> (TempDir, OsString) {
    let dir = TempDir::new();
    for name in names {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        write_script(&dir.path().join(file), "exit 0");
    }
    let path = OsString::from(dir.path());
    (dir, path)
}

// FU-094: `gh` absent is reported by name with what to install.
#[test]
fn fu_094_a_missing_gh_is_named_with_what_to_install() {
    let (_dir, path) = path_with(&["glab"]);
    assert_eq!(
        missing_path_tools(Some(&path)),
        vec![MissingTool {
            command: "gh".to_owned(),
            install: "GitHub CLI".to_owned(),
        }]
    );
}

// FU-094: every tool present reports nothing.
#[test]
fn fu_094_tools_present_report_nothing() {
    let (_dir, path) = path_with(&["gh", "glab"]);
    assert!(missing_path_tools(Some(&path)).is_empty());
}

// FU-094: no `PATH` at all names both tools; a file that is not executable does not count.
#[test]
fn fu_094_no_path_or_a_plain_file_names_both() {
    assert_eq!(missing_path_tools(None).len(), 2);
    let dir = TempDir::new();
    std::fs::write(dir.path().join("gh"), "not a program").expect("file");
    let path = OsString::from(dir.path());
    let missing: Vec<String> = missing_path_tools(Some(&path))
        .into_iter()
        .map(|m| m.command)
        .collect();
    assert_eq!(missing, ["gh", "glab"]);
    // Unix only: Windows has no execute bit, so any file is a program there.
    #[cfg(unix)]
    assert!(find_on_path(Some(&path), &["gh"]).is_none());
}

// Unix only: the stand-in Claude Code is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn the_claude_code_probe_reads_version_and_sign_in() {
    let (dir, path) = path_with(&[]);
    write_script(
        &dir.path().join("claude"),
        r#"case "$1" in
  --version) echo '2.1.302 (Claude Code)' ;;
  auth) echo '{"loggedIn": true, "authMethod": "claude.ai"}' ;;
esac"#,
    );
    let probe = ClaudeCodeProbe::with_path(path);
    let found = probe.locate().await.expect("locate").expect("found");
    assert_eq!(found.version, "2.1.302");
    assert_eq!(found.path, dir.path().join("claude"));
    assert_eq!(
        probe.sign_in_status().await.expect("status"),
        HarnessSignIn::SignedIn {
            method: Some("claude.ai".to_owned())
        }
    );
}

// SET-R1-2: Claude Code reads its sign-in from the Keychain under the account name, so the probe
// passes `USER` on; without it a signed-in Claude Code answers `loggedIn: false`.
#[cfg(unix)]
#[tokio::test]
async fn the_claude_code_probe_passes_the_account_name() {
    let (dir, path) = path_with(&[]);
    write_script(
        &dir.path().join("claude"),
        r#"case "$1" in
  --version) echo '2.1.321' ;;
  auth) if [ -n "$USER" ]; then echo '{"loggedIn": true, "authMethod": "claude.ai"}'; else echo '{"loggedIn": false}'; fi ;;
esac"#,
    );
    if std::env::var_os("USER").is_none() {
        return; // a test runner with no account name cannot show the difference
    }
    let probe = ClaudeCodeProbe::with_path(path);
    assert_eq!(
        probe.sign_in_status().await.expect("status"),
        HarnessSignIn::SignedIn {
            method: Some("claude.ai".to_owned())
        }
    );
}

// Unix only: the stand-in Claude Code is a shell script.
#[cfg(unix)]
#[tokio::test]
async fn the_claude_code_probe_reports_signed_out_and_absent() {
    let (dir, path) = path_with(&[]);
    write_script(
        &dir.path().join("claude"),
        r#"case "$1" in
  --version) echo '2.1.302' ;;
  auth) echo '{"loggedIn": false}' ;;
esac"#,
    );
    let probe = ClaudeCodeProbe::with_path(path);
    assert_eq!(
        probe.sign_in_status().await.expect("status"),
        HarnessSignIn::SignedOut
    );
    let (_empty, none) = path_with(&[]);
    let absent = ClaudeCodeProbe::with_path(none);
    assert!(absent.locate().await.expect("locate").is_none());
}

// SET-R1-4: on Windows the probe's environment carries Windows' own folders, without which
// `claude --version` cannot start; no credential variable is ever among them.
// spec: ordin-desktop/components/SetupAssistant#SET-R1-4
#[cfg(windows)]
#[test]
fn the_probe_environment_carries_windows_own_folders() {
    let names: Vec<&str> = crate::process::minimal_env(None)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(names.contains(&"SystemRoot"), "{names:?}");
    for credential in ["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"] {
        assert!(!names.contains(&credential), "{names:?}");
    }
}
