//! The program's own settings in `<data root>/preferences.json` (OrdinDesktop.md *Data Model*,
//! *Commands this program answers itself*: `settings.get` and `settings.change` of the sections
//! `notifications` and `display`, and `about`, DSK-UP-9).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ordin_service_api::commands::{SettingView, SettingsSection, SettingsView};
use ordin_types::common::SetBy;
use serde::{Deserialize, Serialize};

use crate::error::{PREFERENCES_UNREADABLE, PREFERENCES_WRITE_FAILED, PREFERENCE_VALUE_INVALID};

/// The only `format` this program reads and writes.
pub const FORMAT: u8 = 1;

/// The program's own settings, kept in preferences.json (2D-3, owner App Shell; stored by the
/// program because the core raises the notifications).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DesktopPreferences {
    pub format: u8,
    pub notifications: NotificationPreferences,
    pub appearance: Appearance,
    /// Settings › Memory's behaviour settings (DSK-MEM-1; amended January 2026; user decision;
    /// Approved by the user January 2026). Absent in a file written before → today's behaviour.
    #[serde(default)]
    pub memory: MemoryPreferences,
    /// Settings › Agents and models (DSK-AGM-1; amended January 2026; user decision; Approved by
    /// the user January 2026). Absent in a file written before → every choice is the default.
    #[serde(default, skip_serializing_if = "is_default_agents")]
    pub agents: crate::agent_settings::AgentPreferences,
    /// The path of the SSH key GitHub's git traffic uses, never the key itself (DSK-SSH-1; amended
    /// January 2026; user decision; Approved by the user January 2026). Absent → none chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_ssh_key: Option<PathBuf>,
}

fn is_default_agents(agents: &crate::agent_settings::AgentPreferences) -> bool {
    *agents == crate::agent_settings::AgentPreferences::default()
}

/// Kept here, enforced by the Memory Service and Ordin Host (`MemorySettings`), which the program
/// gives them at start and after every change.
// spec: ordin-desktop/components/OrdinDesktop#DSK-MEM-1
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryPreferences {
    pub keep: KeepMode,
    pub learn_from_conversations: bool,
    pub learn_from_documents: bool,
    /// `None`: Ordin judges from how often a fact is used.
    pub suggest_removal_after_days: Option<u16>,
    pub use_everywhere: bool,
    pub say_which_facts: bool,
}

pub use ordin_memory_service::KeepMode;

impl Default for MemoryPreferences {
    fn default() -> MemoryPreferences {
        MemoryPreferences::from(ordin_memory_service::MemorySettings::default())
    }
}

impl From<ordin_memory_service::MemorySettings> for MemoryPreferences {
    fn from(s: ordin_memory_service::MemorySettings) -> MemoryPreferences {
        MemoryPreferences {
            keep: s.keep,
            learn_from_conversations: s.learn_from_conversations,
            learn_from_documents: s.learn_from_documents,
            suggest_removal_after_days: s.suggest_removal_after_days,
            use_everywhere: s.use_everywhere,
            say_which_facts: s.say_which_facts,
        }
    }
}

impl MemoryPreferences {
    pub fn settings(&self) -> ordin_memory_service::MemorySettings {
        ordin_memory_service::MemorySettings {
            keep: self.keep,
            learn_from_conversations: self.learn_from_conversations,
            learn_from_documents: self.learn_from_documents,
            suggest_removal_after_days: self.suggest_removal_after_days,
            use_everywhere: self.use_everywhere,
            say_which_facts: self.say_which_facts,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NotificationPreferences {
    pub approvals: bool,
    pub finished_work: bool,
    pub questions: bool,
    pub draft_reviews: bool,
    pub handovers: bool,
}

/// Default `System` (11 §4 Settings › Display).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Default for DesktopPreferences {
    fn default() -> DesktopPreferences {
        DesktopPreferences {
            format: FORMAT,
            notifications: NotificationPreferences {
                approvals: true,
                finished_work: true,
                questions: true,
                draft_reviews: true,
                handovers: true,
            },
            appearance: Appearance::System,
            memory: MemoryPreferences::default(),
            agents: crate::agent_settings::AgentPreferences::default(),
            github_ssh_key: None,
        }
    }
}

/// The notification keys of `settings.change`, in the order `settings.get` lists them.
const NOTIFICATION_KEYS: [&str; 5] = [
    "notifications.approvals",
    "notifications.finished_work",
    "notifications.questions",
    "notifications.draft_reviews",
    "notifications.handovers",
];
pub const APPEARANCE_KEY: &str = "display.appearance";
/// The Settings › Memory keys, in the order `settings.get` lists them (DSK-MEM-1).
pub const MEMORY_KEYS: [&str; 6] = [
    "memory.keep_mode",
    "memory.learn_from_conversations",
    "memory.learn_from_documents",
    "memory.suggest_removal_after_days",
    "memory.use_everywhere",
    "memory.say_which_facts",
];
/// The longest "suggest removing after" period, as the Memory Service allows.
const REMOVAL_DAYS_MAX: u16 = 3650;

/// A refused `settings.change`, by its Error Table code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceError {
    /// `DSK_PREFERENCE_VALUE_INVALID`, permanent.
    ValueInvalid,
    /// `DSK_PREFERENCES_WRITE_FAILED`, retryable.
    WriteFailed,
}

impl PreferenceError {
    pub fn code(self) -> &'static str {
        match self {
            PreferenceError::ValueInvalid => PREFERENCE_VALUE_INVALID,
            PreferenceError::WriteFailed => PREFERENCES_WRITE_FAILED,
        }
    }
}

/// What a successful `settings.change` changed.
#[derive(Debug, Clone)]
pub struct Changed {
    pub view: SettingView,
    /// `Some` when `display.appearance` changed, for the `ordin://appearance` event.
    pub appearance: Option<Appearance>,
    /// `Some` when a Settings › Memory key changed: the settings to give the Memory Service.
    pub memory: Option<MemoryPreferences>,
}

/// `preferences.json`: read on each `settings.get`, replaced atomically on each change.
pub struct Preferences {
    path: PathBuf,
    /// Serialises read-modify-write of the file.
    write: Mutex<()>,
    /// `DSK_PREFERENCES_UNREADABLE` is logged once per process.
    unreadable_logged: AtomicBool,
}

impl Preferences {
    pub fn new(path: PathBuf) -> Preferences {
        Preferences {
            path,
            write: Mutex::new(()),
            unreadable_logged: AtomicBool::new(false),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored preferences. Absent file → defaults. Unreadable or invalid file (bad JSON, a
    /// `format` other than 1, an unknown field) → defaults, the file left in place, and
    /// `DSK_PREFERENCES_UNREADABLE` logged once.
    pub fn read(&self) -> DesktopPreferences {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return DesktopPreferences::default()
            }
            Err(_) => return self.unreadable(),
        };
        match serde_json::from_slice::<DesktopPreferences>(&bytes) {
            Ok(prefs) if prefs.format == FORMAT => prefs,
            _ => self.unreadable(),
        }
    }

    fn unreadable(&self) -> DesktopPreferences {
        if !self.unreadable_logged.swap(true, Ordering::SeqCst) {
            tracing::warn!(
                component = "desktop",
                code = PREFERENCES_UNREADABLE,
                "preferences_unreadable"
            );
        }
        DesktopPreferences::default()
    }

    /// `settings.get` of a section this file answers: `notifications`, `display`, and `about`, which
    /// holds no setting of its own (DSK-UP-9). `None` for every other section.
    pub fn section(&self, section: SettingsSection) -> Option<SettingsView> {
        let settings = match section {
            SettingsSection::Notifications => {
                let n = self.read().notifications;
                let values = [
                    n.approvals,
                    n.finished_work,
                    n.questions,
                    n.draft_reviews,
                    n.handovers,
                ];
                // Only the kinds the program raises are listed: a switch for a notice that is never
                // shown would do nothing (amended January 2026; user decision; Approved by the user
                // January 2026).
                // spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
                NOTIFICATION_KEYS
                    .iter()
                    .zip(values)
                    .filter(|(key, _)| crate::notifier::RAISED.contains(key))
                    .map(|(key, value)| view(key, serde_json::Value::Bool(value)))
                    .collect()
            }
            SettingsSection::Display => vec![view(
                APPEARANCE_KEY,
                appearance_value(self.read().appearance),
            )],
            // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-9
            SettingsSection::About => Vec::new(),
            // spec: ordin-desktop/components/OrdinDesktop#DSK-MEM-1
            SettingsSection::WhatOrdinRemembers => {
                let m = self.read().memory;
                let values = [
                    serde_json::to_value(m.keep).unwrap_or(serde_json::Value::Null),
                    serde_json::Value::Bool(m.learn_from_conversations),
                    serde_json::Value::Bool(m.learn_from_documents),
                    m.suggest_removal_after_days
                        .map_or(serde_json::Value::Null, serde_json::Value::from),
                    serde_json::Value::Bool(m.use_everywhere),
                    serde_json::Value::Bool(m.say_which_facts),
                ];
                MEMORY_KEYS
                    .iter()
                    .zip(values)
                    .map(|(key, value)| view(key, value))
                    .collect()
            }
            // DSK-REC-37: the screens read every other section through its owner's commands.
            // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-37
            _ => Vec::new(),
        };
        Some(SettingsView { section, settings })
    }

    /// Whether `key` is one of this file's keys.
    pub fn owns(key: &str) -> bool {
        key == APPEARANCE_KEY || NOTIFICATION_KEYS.contains(&key) || MEMORY_KEYS.contains(&key)
    }

    /// `settings.change` of one of this file's keys: the value checked, then the whole file written
    /// atomically (`preferences.json.tmp`, `fsync`, rename; mode `0600` on Unix).
    pub fn change(&self, key: &str, value: &serde_json::Value) -> Result<Changed, PreferenceError> {
        let _guard = self.write.lock().unwrap_or_else(|p| p.into_inner());
        let mut prefs = self.read();
        let mut appearance = None;
        let mut memory = None;
        if MEMORY_KEYS.contains(&key) {
            change_memory(&mut prefs.memory, key, value)?;
            memory = Some(prefs.memory);
        } else if key == APPEARANCE_KEY {
            let chosen: Appearance =
                serde_json::from_value(value.clone()).map_err(|_| PreferenceError::ValueInvalid)?;
            prefs.appearance = chosen;
            appearance = Some(chosen);
        } else {
            let on = value.as_bool().ok_or(PreferenceError::ValueInvalid)?;
            let n = &mut prefs.notifications;
            let slot = match key {
                "notifications.approvals" => &mut n.approvals,
                "notifications.finished_work" => &mut n.finished_work,
                "notifications.questions" => &mut n.questions,
                "notifications.draft_reviews" => &mut n.draft_reviews,
                "notifications.handovers" => &mut n.handovers,
                _ => return Err(PreferenceError::ValueInvalid),
            };
            *slot = on;
        }
        self.write_atomically(&prefs)
            .map_err(|_| PreferenceError::WriteFailed)?;
        Ok(Changed {
            view: view(key, value.clone()),
            appearance,
            memory,
        })
    }

    /// Settings › Agents and models: one change checked, then the whole file written atomically.
    /// Answers the choices as now stored.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
    pub fn change_agents(
        &self,
        change: &ordin_service_api::commands::ChangeAgentSetting,
    ) -> Result<crate::agent_settings::AgentPreferences, PreferenceError> {
        let _guard = self.write.lock().unwrap_or_else(|p| p.into_inner());
        let mut prefs = self.read();
        prefs.agents.apply(change)?;
        self.write_atomically(&prefs)
            .map_err(|_| PreferenceError::WriteFailed)?;
        Ok(prefs.agents)
    }

    /// Settings › Sign-ins: the SSH key GitHub's git traffic uses, by path, or `None` (DSK-SSH-1).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
    pub fn set_github_ssh_key(&self, key: Option<PathBuf>) -> Result<(), PreferenceError> {
        let _guard = self.write.lock().unwrap_or_else(|p| p.into_inner());
        let mut prefs = self.read();
        prefs.github_ssh_key = key;
        self.write_atomically(&prefs)
            .map_err(|_| PreferenceError::WriteFailed)
    }

    fn write_atomically(&self, prefs: &DesktopPreferences) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(prefs).map_err(std::io::Error::other)?;
        let tmp = self.path.with_file_name("preferences.json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options.open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}

/// One Settings › Memory key's value checked (DSK-MEM-1); a wrong type or a period outside 1–3,650
/// days is `DSK_PREFERENCE_VALUE_INVALID`.
fn change_memory(
    m: &mut MemoryPreferences,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), PreferenceError> {
    let flag = || value.as_bool().ok_or(PreferenceError::ValueInvalid);
    match key {
        "memory.keep_mode" => {
            m.keep =
                serde_json::from_value(value.clone()).map_err(|_| PreferenceError::ValueInvalid)?;
        }
        "memory.learn_from_conversations" => m.learn_from_conversations = flag()?,
        "memory.learn_from_documents" => m.learn_from_documents = flag()?,
        "memory.suggest_removal_after_days" => {
            m.suggest_removal_after_days = match value {
                serde_json::Value::Null => None,
                v => {
                    let days = v
                        .as_u64()
                        .and_then(|d| u16::try_from(d).ok())
                        .filter(|d| (1..=REMOVAL_DAYS_MAX).contains(d))
                        .ok_or(PreferenceError::ValueInvalid)?;
                    Some(days)
                }
            };
        }
        "memory.use_everywhere" => m.use_everywhere = flag()?,
        "memory.say_which_facts" => m.say_which_facts = flag()?,
        _ => return Err(PreferenceError::ValueInvalid),
    }
    Ok(())
}

fn appearance_value(appearance: Appearance) -> serde_json::Value {
    serde_json::to_value(appearance).unwrap_or(serde_json::Value::Null)
}

fn view(key: &str, value: serde_json::Value) -> SettingView {
    SettingView {
        key: key.to_owned(),
        value,
        set_by: SetBy::Person,
        person_may_change: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use serde_json::json;

    fn prefs(dir: &TempDir) -> Preferences {
        Preferences::new(dir.path().join("preferences.json"))
    }

    #[test]
    fn an_absent_file_gives_the_defaults_and_nothing_is_written() {
        let dir = TempDir::new("prefs-absent");
        let p = prefs(&dir);
        assert_eq!(p.read(), DesktopPreferences::default());
        let view = p.section(SettingsSection::Notifications).expect("answered");
        assert_eq!(view.settings.len(), crate::notifier::RAISED.len());
        assert!(view.settings.iter().all(|s| s.value == json!(true)));
        let display = p.section(SettingsSection::Display).expect("answered");
        assert_eq!(display.settings[0].key, "display.appearance");
        assert_eq!(display.settings[0].value, json!("system"));
        assert!(!p.path().exists(), "nothing is written by a read");
    }

    // Gherkin (ADR-114): Happy path — Settings › About has no setting of its own.
    #[test]
    fn about_has_no_setting_of_its_own() {
        let dir = TempDir::new("prefs-about");
        let view = prefs(&dir)
            .section(SettingsSection::About)
            .expect("answered");
        assert_eq!(view.section, SettingsSection::About);
        assert!(view.settings.is_empty());
        let history = prefs(&dir)
            .section(SettingsSection::History)
            .expect("answered");
        assert!(history.settings.is_empty(), "DSK-REC-37");
        assert!(!Preferences::owns("about.version"));
    }

    #[test]
    fn a_change_is_written_and_read_back() {
        let dir = TempDir::new("prefs-change");
        let p = prefs(&dir);
        let changed = p
            .change("notifications.approvals", &json!(false))
            .expect("saved");
        assert_eq!(changed.view.key, "notifications.approvals");
        assert_eq!(changed.appearance, None);
        let changed = p.change(APPEARANCE_KEY, &json!("dark")).expect("saved");
        assert_eq!(changed.appearance, Some(Appearance::Dark));
        let read = p.read();
        assert!(!read.notifications.approvals && read.notifications.questions);
        assert_eq!(read.appearance, Appearance::Dark);
        let on_disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(p.path()).expect("file")).expect("json");
        assert_eq!(on_disk["format"], json!(1));
        assert!(!dir.path().join("preferences.json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(p.path())
                .expect("file")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    // Error Table: a value of the wrong type.
    #[test]
    fn error_path_a_value_of_the_wrong_type() {
        let dir = TempDir::new("prefs-invalid");
        let p = prefs(&dir);
        for (key, value) in [
            ("notifications.approvals", json!("yes")),
            (APPEARANCE_KEY, json!("blue")),
            (APPEARANCE_KEY, json!(true)),
        ] {
            let err = p.change(key, &value).expect_err("refused");
            assert_eq!(err.code(), "DSK_PREFERENCE_VALUE_INVALID");
        }
        assert!(!p.path().exists());
    }

    // Gherkin: Error path — preferences file damaged.
    #[test]
    fn error_path_preferences_file_damaged() {
        let dir = TempDir::new("prefs-damaged");
        let p = prefs(&dir);
        for bad in [
            &b"{not json"[..],
            br#"{"format":2,"notifications":{"approvals":true,"finished_work":true,"questions":true,"draft_reviews":true,"handovers":true},"appearance":"system"}"#,
            br#"{"format":1,"extra":1,"notifications":{"approvals":true,"finished_work":true,"questions":true,"draft_reviews":true,"handovers":true},"appearance":"system"}"#,
        ] {
            std::fs::write(p.path(), bad).expect("write");
            assert_eq!(p.read(), DesktopPreferences::default());
            assert_eq!(std::fs::read(p.path()).expect("left in place"), bad);
        }
        assert!(p.unreadable_logged.load(Ordering::SeqCst));
        // The next write replaces it.
        p.change("notifications.questions", &json!(false))
            .expect("saved");
        assert!(!p.read().notifications.questions);
    }

    // Gherkin: Error path — preferences cannot be saved.
    #[test]
    fn error_path_preferences_cannot_be_saved() {
        let dir = TempDir::new("prefs-unwritable");
        // A file where the data root should be: nothing can be written under it, whoever runs.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"").expect("blocker");
        let p = Preferences::new(blocker.join("preferences.json"));
        let err = p
            .change("notifications.approvals", &json!(false))
            .expect_err("refused");
        assert_eq!(err, PreferenceError::WriteFailed);
        assert_eq!(err.code(), "DSK_PREFERENCES_WRITE_FAILED");
    }

    // DSK-MEM-1 (amended January 2026; user decision; Approved by the user January 2026).
    #[test]
    fn memory_settings_default_to_todays_behaviour_and_an_older_file_reads() {
        let dir = TempDir::new("prefs-memory-default");
        let p = prefs(&dir);
        let view = p
            .section(SettingsSection::WhatOrdinRemembers)
            .expect("answered");
        let pairs: Vec<(String, serde_json::Value)> = view
            .settings
            .iter()
            .map(|s| (s.key.clone(), s.value.clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("memory.keep_mode".to_owned(), json!("on_its_own")),
                ("memory.learn_from_conversations".to_owned(), json!(true)),
                ("memory.learn_from_documents".to_owned(), json!(true)),
                ("memory.suggest_removal_after_days".to_owned(), json!(null)),
                ("memory.use_everywhere".to_owned(), json!(true)),
                ("memory.say_which_facts".to_owned(), json!(false)),
            ]
        );
        // A file written before Settings › Memory had settings.
        std::fs::write(
            p.path(),
            br#"{"format":1,"notifications":{"approvals":false,"finished_work":true,"questions":true,"draft_reviews":true,"handovers":true},"appearance":"dark"}"#,
        )
        .expect("write");
        let read = p.read();
        assert!(!read.notifications.approvals);
        assert_eq!(read.memory, MemoryPreferences::default());
        assert_eq!(
            read.memory.settings(),
            ordin_memory_service::MemorySettings::default()
        );
    }

    #[test]
    fn memory_settings_change_and_are_handed_on() {
        let dir = TempDir::new("prefs-memory-change");
        let p = prefs(&dir);
        for (key, value) in [
            ("memory.keep_mode", json!("ask_first")),
            ("memory.learn_from_conversations", json!(false)),
            ("memory.learn_from_documents", json!(false)),
            ("memory.suggest_removal_after_days", json!(90)),
            ("memory.use_everywhere", json!(false)),
            ("memory.say_which_facts", json!(true)),
        ] {
            let changed = p.change(key, &value).expect("saved");
            assert_eq!(changed.view.value, value);
            assert!(changed.memory.is_some(), "{key} is handed on");
            assert_eq!(changed.appearance, None);
        }
        let s = p.read().memory.settings();
        assert_eq!(s.keep, KeepMode::AskFirst);
        assert!(!s.learn_from_conversations && !s.learn_from_documents && !s.use_everywhere);
        assert_eq!(s.suggest_removal_after_days, Some(90));
        assert!(s.say_which_facts);
        // Back to "Ordin decides".
        p.change("memory.suggest_removal_after_days", &json!(null))
            .expect("saved");
        assert_eq!(p.read().memory.suggest_removal_after_days, None);
        // A notification change hands nothing to memory.
        assert!(p
            .change("notifications.approvals", &json!(false))
            .expect("saved")
            .memory
            .is_none());
    }

    #[test]
    fn error_path_a_memory_value_out_of_bounds_or_of_the_wrong_type() {
        let dir = TempDir::new("prefs-memory-invalid");
        let p = prefs(&dir);
        for (key, value) in [
            ("memory.keep_mode", json!("sometimes")),
            ("memory.keep_mode", json!(true)),
            ("memory.learn_from_documents", json!("no")),
            ("memory.suggest_removal_after_days", json!(0)),
            ("memory.suggest_removal_after_days", json!(3651)),
            ("memory.suggest_removal_after_days", json!(-5)),
            ("memory.suggest_removal_after_days", json!("90")),
            ("memory.say_which_facts", json!(1)),
        ] {
            let err = p.change(key, &value).expect_err("refused");
            assert_eq!(err.code(), "DSK_PREFERENCE_VALUE_INVALID", "{key} {value}");
        }
        assert!(!p.path().exists(), "nothing is written");
        assert!(Preferences::owns("memory.keep_mode"));
        assert!(!Preferences::owns("memory.unknown"));
    }

    // DSK-AGM-1: Settings › Agents and models are kept beside the other settings.
    #[test]
    fn agent_choices_are_kept_and_an_older_file_reads() {
        use ordin_service_api::commands::profile::{HarnessName, WorkKind};
        use ordin_service_api::commands::ChangeAgentSetting;
        let dir = TempDir::new("prefs-agents");
        let p = prefs(&dir);
        assert_eq!(
            p.read().agents,
            crate::agent_settings::AgentPreferences::default()
        );
        let stored = p
            .change_agents(&ChangeAgentSetting::Effort {
                harness: HarnessName::ClaudeCode,
                kind: WorkKind::Code,
                effort: Some("high".to_owned()),
            })
            .expect("stored");
        assert_eq!(p.read().agents, stored);
        assert_eq!(
            stored.choices().effort[&ordin_organisation_profile::HarnessName::ClaudeCode]
                [&ordin_organisation_profile::WorkKind::Code],
            "high"
        );
        // A refused value writes nothing.
        let err = p
            .change_agents(&ChangeAgentSetting::Effort {
                harness: HarnessName::ClaudeCode,
                kind: WorkKind::Code,
                effort: Some("extreme".to_owned()),
            })
            .expect_err("refused");
        assert_eq!(err.code(), "DSK_PREFERENCE_VALUE_INVALID");
        assert_eq!(p.read().agents, stored);
        // A file written before the section reads with every choice at its default.
        let older = DesktopPreferences::default();
        let mut json = serde_json::to_value(&older).unwrap();
        json.as_object_mut().unwrap().remove("agents");
        std::fs::write(p.path(), serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(p.read(), older);
    }
}
