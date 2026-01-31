//! The service's files under the minutes folder (MinutesService.md *Paths* and *Data Model*), each
//! written atomically: a `.part` file beside it, flushed, then renamed over it.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ordin_types::id::{MinutesId, MinutesJobId, RecordingId, WorkspaceId};
use ordin_types::time::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::MinutesError;
use crate::types::{MinutesSettings, RecordingView, SpeechModelSetting};

pub(crate) const SETTINGS_FILE: &str = "settings.json";
pub(crate) const RECORDING_FILE: &str = "recording.json";
pub(crate) const DEFAULT_OBS_PORT: u16 = 4455;
pub(crate) const LOWEST_PORT: u16 = 1024;

/// `settings.json`, `format` 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SettingsFile {
    pub format: u32,
    /// A file written before MIN-MACH-1 keeps its `larger` or `smaller`, read as the person's
    /// choice; any value outside the three makes the file unparsable (A1).
    // spec: ordin-desktop/components/MinutesService#MIN-MACH-1
    pub speech_model: SpeechModelSetting,
    pub delete_recordings_after_save: bool,
    pub obs_port: u16,
    pub model_consent_at: Option<Timestamp>,
    pub writer_notice_seen_at: Option<Timestamp>,
}

impl Default for SettingsFile {
    fn default() -> Self {
        SettingsFile {
            format: 1,
            speech_model: SpeechModelSetting::Recommended,
            delete_recordings_after_save: false,
            obs_port: DEFAULT_OBS_PORT,
            model_consent_at: None,
            writer_notice_seen_at: None,
        }
    }
}

impl SettingsFile {
    pub(crate) fn settings(&self) -> MinutesSettings {
        MinutesSettings {
            speech_model: self.speech_model,
            delete_recordings_after_save: self.delete_recordings_after_save,
            obs_port: self.obs_port,
        }
    }

    fn valid(&self) -> bool {
        self.format == 1 && self.obs_port >= LOWEST_PORT
    }
}

/// `recording.json`: the running recording and the folder OBS records into (D1, for D4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecordingFile {
    pub format: u32,
    pub id: RecordingId,
    pub workspace_id: WorkspaceId,
    pub started_at: Timestamp,
    pub others_heard: bool,
    pub record_folder: PathBuf,
}

impl RecordingFile {
    pub(crate) fn view(&self) -> RecordingView {
        RecordingView {
            id: self.id,
            workspace_id: self.workspace_id,
            started_at: self.started_at,
            others_heard: self.others_heard,
        }
    }
}

/// The folders of one workspace.
pub(crate) struct WorkspaceFolders {
    pub root: PathBuf,
}

impl WorkspaceFolders {
    pub(crate) fn of(minutes_folder: &Path, workspace_id: WorkspaceId) -> WorkspaceFolders {
        WorkspaceFolders {
            root: minutes_folder.join(workspace_id.to_string()),
        }
    }

    pub(crate) fn recordings(&self) -> PathBuf {
        self.root.join("recordings")
    }

    pub(crate) fn transcripts(&self) -> PathBuf {
        self.root.join("transcripts")
    }

    pub(crate) fn recording(&self, id: RecordingId) -> PathBuf {
        self.recordings().join(format!("{id}.mkv"))
    }

    pub(crate) fn job_transcript(&self, id: MinutesJobId) -> PathBuf {
        self.transcripts().join(format!("{id}.json"))
    }

    pub(crate) fn minutes_transcript(&self, id: MinutesId) -> PathBuf {
        self.transcripts().join(format!("{id}.json"))
    }
}

/// A1: absent → defaults; unparsable or outside its values → renamed to `settings.json.bad`,
/// defaults used, `minutes.settings_reset` logged.
pub(crate) fn read_settings(folder: &Path) -> SettingsFile {
    let path = folder.join(SETTINGS_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return SettingsFile::default(),
    };
    match serde_json::from_slice::<SettingsFile>(&bytes) {
        Ok(settings) if settings.valid() => settings,
        _ => {
            let _ = std::fs::rename(&path, folder.join("settings.json.bad"));
            tracing::warn!(component = "minutes", "minutes.settings_reset");
            SettingsFile::default()
        }
    }
}

/// Writes `bytes` to `path` atomically; any failure → `StoreFailed`, and no `.part` file is left.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), MinutesError> {
    let part = part_of(path);
    let written = (|| {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&part, path)
    })();
    written.map_err(|_| {
        let _ = std::fs::remove_file(&part);
        MinutesError::StoreFailed
    })
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), MinutesError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|_| MinutesError::StoreFailed)?;
    write_atomic(path, &bytes)
}

fn part_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// A1: every `*.part` file under the minutes folder, left by a crash, is deleted.
pub(crate) fn remove_parts(folder: &Path) {
    for file in files_under(folder) {
        if file.extension().is_some_and(|e| e == "part") {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// Every file under `folder`, at any depth; unreadable folders are skipped.
pub(crate) fn files_under(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => pending.push(path),
                Ok(t) if t.is_file() => found.push(path),
                _ => {}
            }
        }
    }
    found
}

/// D2: rename into place; across volumes copy, `fsync`, then delete the original.
pub(crate) fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let copied = (|| {
        std::fs::copy(from, to)?;
        std::fs::File::open(to)?.sync_all()
    })();
    if let Err(error) = copied {
        let _ = std::fs::remove_file(to);
        return Err(error);
    }
    std::fs::remove_file(from)
}

/// D4: the newest `.mkv` directly in `folder` modified after `after`.
pub(crate) fn newest_recording(folder: &Path, after: Timestamp) -> Option<PathBuf> {
    let after = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(after.unix_millis()).unwrap_or(0));
    std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("mkv"))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(modified, _)| *modified > after)
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Deletes a file; absent counts as deleted.
pub(crate) fn remove_if_present(path: &Path) -> Result<(), MinutesError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(MinutesError::StoreFailed),
    }
}
