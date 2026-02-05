//! Table-driven tests of the crate's pure rules (version token, archive paths, update addresses,
//! signatures), FU-094's `PATH` check and the built-in Claude Code probe against a
//! stand-in `claude` script.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::process::{claude_code_folders, path_with_folder, version_token};
#[cfg(unix)]
use crate::process::{find_claude_code, find_on_path, ClaudeCodeProbe};
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

// SET-R1-7, SET-R1-8: a Finder or Dock start gets a `PATH` without the person's own `~/.local/bin`;
// Claude Code installed there is still found, its version read, and its own folder is on the `PATH`
// its commands get, so what it starts from beside itself is found too.
// spec: ordin-desktop/components/SetupAssistant#SET-R1-7
// spec: ordin-desktop/components/SetupAssistant#SET-R1-8
#[cfg(unix)]
#[tokio::test]
async fn the_claude_code_probe_finds_claude_in_the_home_local_bin() {
    let (_short, path) = path_with(&[]);
    let home = TempDir::new();
    let bin = home.path().join(".local").join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    write_script(
        &bin.join("claude"),
        r#"case "$1" in
  --version) echo '2.1.330 (Claude Code)' ;;
  auth) if command -v claude >/dev/null 2>&1; then echo '{"loggedIn": true, "authMethod": "claude.ai"}'; else echo '{"loggedIn": false}'; fi ;;
esac"#,
    );
    // Without the home folder the `PATH` alone does not hold it.
    assert!(ClaudeCodeProbe::with_path(path.clone())
        .locate()
        .await
        .expect("locate")
        .is_none());
    let probe = ClaudeCodeProbe::with_path_and_home(path, home.path());
    let found = probe.locate().await.expect("locate").expect("found");
    assert_eq!(found.path, bin.join("claude"));
    assert_eq!(found.version, "2.1.330");
    assert_eq!(
        probe.sign_in_status().await.expect("status"),
        HarnessSignIn::SignedIn {
            method: Some("claude.ai".to_owned())
        }
    );
}

// SET-R1-7: `PATH` comes first, and a home folder with no `claude` finds nothing.
#[cfg(unix)]
#[test]
fn find_claude_code_takes_path_first_then_the_home_local_bin() {
    let (on_path, path) = path_with(&["claude"]);
    let home = TempDir::new();
    let bin = home.path().join(".local").join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    assert_eq!(
        find_claude_code(Some(&path), Some(home.path())),
        Some(on_path.path().join("claude"))
    );
    write_script(&bin.join("claude"), "exit 0");
    assert_eq!(
        find_claude_code(Some(&path), Some(home.path())),
        Some(on_path.path().join("claude"))
    );
    let (_empty, none) = path_with(&[]);
    assert_eq!(
        find_claude_code(Some(&none), Some(home.path())),
        Some(bin.join("claude"))
    );
    assert_eq!(
        find_claude_code(None, Some(home.path())),
        Some(bin.join("claude"))
    );
    let empty_home = TempDir::new();
    assert_eq!(find_claude_code(Some(&none), Some(empty_home.path())), None);
}

// SET-R1-7: the folders after `PATH` start with the home folder's `.local/bin`, and on macOS add
// Homebrew's two folders; every one is absolute.
#[test]
fn the_claude_code_folders_start_with_the_home_local_bin() {
    let home = Path::new(if cfg!(windows) {
        r"C:\Users\a"
    } else {
        "/home/a"
    });
    let folders = claude_code_folders(Some(home));
    assert_eq!(folders[0], home.join(".local").join("bin"));
    assert!(claude_code_folders(None).iter().all(|f| f.is_absolute()));
    if cfg!(target_os = "macos") {
        assert!(folders.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(folders.contains(&PathBuf::from("/usr/local/bin")));
    } else {
        assert_eq!(folders.len(), 1);
    }
}

// SET-R1-8: the folder is added once, at the end, and only when it is missing.
#[test]
fn the_child_path_gains_the_folder_claude_was_found_in() {
    let dir = TempDir::new();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let joined = |entries: &[&Path]| std::env::join_paths(entries).expect("join");
    let path = Some(joined(&[&a]));
    assert_eq!(
        path_with_folder(path.clone(), Some(&b)),
        Some(joined(&[&a, &b]))
    );
    assert_eq!(path_with_folder(path.clone(), Some(&a)), path);
    assert_eq!(path_with_folder(None, Some(&b)), Some(joined(&[&b])));
    assert_eq!(path_with_folder(path.clone(), None), path);
    assert_eq!(path_with_folder(path.clone(), Some(Path::new(""))), path);
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

// SET-CPX-2: `copilot` on `PATH` first, then the home folder's `.local/bin`; nothing found is `None`.
#[cfg(unix)]
#[test]
fn find_copilot_cli_takes_path_first_then_the_home_local_bin() {
    use crate::process::find_copilot_cli;
    let (on_path, path) = path_with(&["copilot"]);
    let home = TempDir::new();
    let bin = home.path().join(".local").join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    write_script(&bin.join("copilot"), "exit 0");
    assert_eq!(
        find_copilot_cli(Some(&path), Some(home.path()), None, None),
        Some(on_path.path().join("copilot"))
    );
    let (_empty, none) = path_with(&[]);
    assert_eq!(
        find_copilot_cli(Some(&none), Some(home.path()), None, None),
        Some(bin.join("copilot"))
    );
    let empty_home = TempDir::new();
    assert_eq!(
        find_copilot_cli(Some(&none), Some(empty_home.path()), None, None),
        None
    );
}

// SET-CPX-2: the folders after `PATH`; Homebrew's on macOS, WinGet's and npm's on Windows, each
// skipped when its variable is absent.
#[test]
fn copilot_cli_folders_follow_the_install_routes() {
    use crate::process::copilot_cli_folders;
    let home = PathBuf::from(if cfg!(windows) {
        "C:/Users/p"
    } else {
        "/home/p"
    });
    let local = PathBuf::from("C:/Users/p/AppData/Local");
    let roaming = PathBuf::from("C:/Users/p/AppData/Roaming");
    let folders = copilot_cli_folders(Some(&home), Some(&local), Some(&roaming));
    assert_eq!(folders[0], home.join(".local").join("bin"));
    if cfg!(target_os = "macos") {
        assert_eq!(
            &folders[1..],
            [
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin")
            ]
        );
    } else if cfg!(windows) {
        assert_eq!(
            &folders[1..],
            [
                local.join("Microsoft").join("WinGet").join("Links"),
                roaming.join("npm")
            ]
        );
        assert_eq!(copilot_cli_folders(None, None, None), Vec::<PathBuf>::new());
    } else {
        assert_eq!(folders.len(), 1);
    }
}

// SET-CPX-7: an npm install under a Node version manager is found, newest version first, in both
// folder layouts; anything that is not a version folder with a `bin` is skipped.
// spec: ordin-desktop/components/SetupAssistant#SET-CPX-7
#[test]
fn copilot_under_a_node_version_manager_is_found_newest_first() {
    use crate::process::nvm_bin_folders;
    let home = TempDir::new();
    let versions = home.path().join(".nvm").join("versions");
    for dir in [
        "node/v20.11.1/bin",
        "node/v24.16.0/bin",
        "v22.3.0/bin",
        "node/not-a-version/bin",
        "v18.0.0",
    ] {
        std::fs::create_dir_all(versions.join(dir)).unwrap();
    }
    assert_eq!(
        nvm_bin_folders(&versions),
        [
            versions.join("node").join("v24.16.0").join("bin"),
            versions.join("v22.3.0").join("bin"),
            versions.join("node").join("v20.11.1").join("bin"),
        ]
    );
    assert!(nvm_bin_folders(&home.path().join("missing")).is_empty());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let copilot = versions
            .join("node")
            .join("v24.16.0")
            .join("bin")
            .join("copilot");
        std::fs::write(&copilot, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&copilot, std::fs::Permissions::from_mode(0o755)).unwrap();
        let none = std::ffi::OsString::new();
        assert_eq!(
            crate::process::find_copilot_cli(Some(&none), Some(home.path()), None, None),
            Some(copilot)
        );
    }
}

// SET-CPX-2: on Windows `copilot.exe` is taken before `copilot.cmd`, and `copilot.cmd` (npm) alone is
// found in `<APPDATA>\npm`.
#[cfg(windows)]
#[test]
fn find_copilot_cli_takes_exe_then_cmd_on_windows() {
    use crate::process::find_copilot_cli;
    let roaming = TempDir::new();
    let npm = roaming.path().join("npm");
    std::fs::create_dir_all(&npm).expect("npm");
    std::fs::write(npm.join("copilot.cmd"), b"@echo 1.0.98").expect("cmd");
    let none = OsString::new();
    assert_eq!(
        find_copilot_cli(Some(&none), None, None, Some(roaming.path())),
        Some(npm.join("copilot.cmd"))
    );
    std::fs::write(npm.join("copilot.exe"), b"").expect("exe");
    assert_eq!(
        find_copilot_cli(Some(&none), None, None, Some(roaming.path())),
        Some(npm.join("copilot.exe"))
    );
}
