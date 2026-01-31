//! The adapter against a real OBS 30 or later on the Windows test machine (plan 07 Phase 9). Ignored:
//! they need OBS installed, the signed-in desktop (OBS draws a window), and `ffprobe` on the PATH, and
//! they start and close OBS. Run in the desktop session with
//! `cargo test -p ordin-obs -- --ignored real_obs --test-threads=1`.
//!
//! OBS's WebSocket server is off until a person turns it on; the tests turn it on, with no password,
//! in OBS's own settings file before starting it, as a person would in Tools › WebSocket Server Settings.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::{identity, localisation, TempDir};
use crate::detect::registry_folder;
use crate::ObsRecorder;

fn obs_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq obs64.exe", "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("obs64.exe"))
        .unwrap_or(false)
}

/// Closes OBS as its window's close button does, so its next start shows no safe-mode question.
fn close_obs() {
    let _ = Command::new("taskkill").args(["/IM", "obs64.exe"]).output();
    let until = Instant::now() + Duration::from_secs(30);
    while obs_running() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(500));
    }
    if obs_running() {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", "obs64.exe"])
            .output();
    }
}

/// Clears the run markers OBS leaves when it was stopped by force, so it starts without asking
/// whether to start in safe mode (that question holds the WebSocket server back until answered).
fn forget_unclean_shutdowns() {
    let sentinel = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
        .join("obs-studio")
        .join(".sentinel");
    let _ = std::fs::remove_dir_all(sentinel);
}

/// OBS's WebSocket server on, port 4455, no password (`%APPDATA%\obs-studio\plugin_config\obs-websocket`).
fn turn_on_websocket() {
    let folder = PathBuf::from(std::env::var_os("APPDATA").expect("APPDATA"))
        .join("obs-studio")
        .join("plugin_config")
        .join("obs-websocket");
    std::fs::create_dir_all(&folder).expect("obs-websocket folder");
    let config = serde_json::json!({
        "alerts_enabled": false,
        "auth_required": false,
        "first_load": false,
        "server_enabled": true,
        "server_password": "",
        "server_port": 4455
    });
    std::fs::write(folder.join("config.json"), config.to_string()).expect("obs-websocket config");
}

/// The audio streams of a recording, as `ffprobe` counts them.
fn audio_streams(file: &Path) -> usize {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .expect("ffprobe on the PATH");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

// FU-010: Open OBS starts the OBS that the registry names.
#[tokio::test]
#[ignore = "needs OBS installed and the signed-in desktop"]
async fn open_obs_starts_a_real_obs_found_through_the_registry() {
    close_obs();
    forget_unclean_shutdowns();
    let folder = registry_folder().expect("HKLM\\SOFTWARE\\OBS Studio names OBS's folder");
    assert!(folder.join("bin").join("64bit").join("obs64.exe").is_file());
    let data = TempDir::new();
    let recorder = ObsRecorder::new(identity(&data.0).await, localisation().await);
    recorder.open_obs().await.expect("open OBS");
    let until = Instant::now() + Duration::from_secs(30);
    while !obs_running() {
        assert!(Instant::now() < until, "OBS did not start within 30 s");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    close_obs();
}

// C-111: a real OBS accepts the profile parameters and input kinds, and records both tracks.
#[tokio::test]
#[ignore = "needs OBS installed, the signed-in desktop and ffprobe"]
async fn a_real_obs_records_the_two_track_file() {
    close_obs();
    forget_unclean_shutdowns();
    turn_on_websocket();
    let data = TempDir::new();
    let recorder = ObsRecorder::new(identity(&data.0).await, localisation().await);
    recorder.open_obs().await.expect("open OBS");
    // OBS takes a while to load its plugins and open the WebSocket server.
    let until = Instant::now() + Duration::from_secs(90);
    let started = loop {
        match recorder.start().await {
            Ok(started) => break started,
            Err(_) if Instant::now() < until => {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(error) => {
                close_obs();
                panic!("OBS did not start recording within 90 s: {error:?}");
            }
        }
    };
    assert!(
        started.others_heard,
        "OBS 30 or later records what others say"
    );
    tokio::time::sleep(Duration::from_secs(5)).await;
    let finished = recorder.stop().await;
    close_obs();
    let finished = finished.expect("stop");
    assert!(finished.output_path.is_file(), "the recording is a file");
    assert_eq!(
        audio_streams(&finished.output_path),
        2,
        "the recording has You and Others"
    );
    let _ = std::fs::remove_file(&finished.output_path);
}
