//! Row 3.33 with `copilot` (plan 08 Phase 7): the Copilot CLI is the person's own (ADR-123). The
//! DSK-CPX scenarios over the desktop's real construction and its `CopilotCliSource` and Copilot
//! `HarnessProbe` (OrdinDesktop.md amendment 2026-02-02).

use std::ffi::OsString;
use std::path::Path;

use ordin_copilot_adapter::CopilotCliSource;
use ordin_setup::{HarnessKind, HarnessProbe};

use crate::hosts_copilot::{CopilotLookup, DesktopCopilotCli, DesktopCopilotProbe};
use crate::plan::{start_steps, Adapter, Row, Step};
use crate::test_support::TempDir;
use crate::wiring_tests::wire_all;

/// A short `PATH` holding no `copilot`, and `home` as the person's home folder.
fn lookup(home: &Path) -> CopilotLookup {
    CopilotLookup {
        path: Some(OsString::from("/usr/bin:/bin:/usr/sbin:/sbin")),
        home: Some(home.to_path_buf()),
        ..CopilotLookup::default()
    }
}

/// A stub `copilot` in `<home>/.local/bin` that prints `version`.
#[cfg(unix)]
fn place_stub(home: &Path, version: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = home.join(".local").join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let file = bin.join("copilot");
    std::fs::write(&file, format!("#!/bin/sh\necho '{version}'\n")).expect("stub");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("mode");
    file
}

// Scenario: Edge case — Copilot on, no Copilot CLI installed (DSK-CPX-2, DSK-CPX-5)
#[tokio::test]
async fn dsk_cpx_2_copilot_on_with_no_copilot_cli_installed_starts() {
    let dir = TempDir::new("cpx-none");
    let runner = wire_all(&dir).await;
    let m = &runner.modules;
    // Start completed with the adapter built and registered after Claude's, and its probe in place.
    assert!(m.copilot.is_some());
    assert_eq!(m.adapters, [Adapter::Claude, Adapter::Copilot]);
    let steps = start_steps();
    let setup = steps
        .iter()
        .position(|s| *s == Step::Construct(Row::Setup))
        .expect("row 3.43");
    assert!(steps[setup..].contains(&Step::RegisterCopilotProbe));
    // No bundled Copilot is looked for: the hook is the only bundled program.
    assert_eq!(crate::plan::bundled_program_names(), ["ordin-hook"]);
    assert!(!runner.inputs.paths.missing_sidecars().contains(&"copilot"));
    // The probe's locate answers None.
    let home = TempDir::new("cpx-home");
    let probe = DesktopCopilotProbe {
        lookup: lookup(home.path()),
        adapter: m.copilot.clone().expect("adapter"),
    };
    assert_eq!(probe.harness(), HarnessKind::CopilotCli);
    assert!(matches!(probe.locate().await, Ok(None)));
    let source = DesktopCopilotCli {
        lookup: lookup(home.path()),
    };
    assert_eq!(source.current().await, None);
}

// Scenario: Happy path — the person's Copilot CLI reaches the adapter (DSK-CPX-4); and SET-CPX-5:
// a CLI placed while ordin is open is found by the next call, with no restart.
#[cfg(unix)]
#[tokio::test]
async fn dsk_cpx_4_the_persons_copilot_cli_reaches_the_adapter() {
    let home = TempDir::new("cpx-found");
    let source = DesktopCopilotCli {
        lookup: lookup(home.path()),
    };
    assert_eq!(source.current().await, None);
    let stub = place_stub(home.path(), "1.0.98");
    let found = source.current().await.expect("found from a short PATH");
    assert_eq!(found.path, stub);
    assert_eq!(found.version, "1.0.98");
}

// DSK-CPX-3: output with no version token is unreadable, not "not installed".
#[cfg(unix)]
#[tokio::test]
async fn dsk_cpx_3_a_copilot_with_no_version_is_unreadable() {
    let home = TempDir::new("cpx-garbled");
    place_stub(home.path(), "no version here");
    let lookup = lookup(home.path());
    assert!(matches!(
        lookup.locate().await,
        Err(ordin_setup::ProbeError::Unreadable)
    ));
}
