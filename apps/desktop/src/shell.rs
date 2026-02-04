//! What the window shell does for the core (OrdinDesktop.md *Commands this program answers itself*,
//! *Internal Logic* steps 5, 10 and 15): the dialogs on the main window, the quit flow and the
//! appearance event. The commands' checks are here, so they are tested without a window; the shell
//! of `main.rs` implements [`Shell`] with Tauri on macOS and Windows.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::core::QuitReason;
use crate::prefs::Appearance;

/// The folder picker could not open (`DSK_PICKER_UNAVAILABLE`, retryable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerUnavailable;

/// What started a quit (step 10, step 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitTrigger {
    /// The person closed the main window (`WindowEvent::CloseRequested`).
    WindowClosed,
    /// The application menu's Quit.
    Menu,
    /// The screens sent `app.quit`.
    AppQuit,
    /// `setup.install_update` succeeded with an offer `Ready` (DSK-UP-10, step 15).
    Update,
    /// The operating system ends the session (`RunEvent::ExitRequested` without a person's quit).
    OperatingSystemEnding,
    /// `setup.choose_harness` changed the harness to use (DSK-CHO-2 step 3). First start is still
    /// drawn, so no session runs and nothing is asked.
    HarnessChosen,
}

/// The window shell's part of the commands and of the quit flow.
#[async_trait::async_trait]
pub trait Shell: Send + Sync {
    /// `app.pick_folder`: the operating system's folder picker on the main window. `Ok(None)` when
    /// the person cancelled.
    async fn pick_folder(&self) -> Result<Option<PathBuf>, PickerUnavailable>;
    /// The operating system's file picker on the main window, with `title` (OrdinDesktop.md
    /// DSK-R1-1). `Ok(None)` when the person cancelled.
    async fn pick_file(&self, title: &str) -> Result<Option<PathBuf>, PickerUnavailable>;
    /// The file picker with one filter, `filter_name` over `extensions` (OrdinDesktop.md DSK-MIN-2).
    /// `Ok(None)` when the person cancelled.
    async fn pick_file_of(
        &self,
        title: &str,
        filter_name: &str,
        extensions: &[&str],
    ) -> Result<Option<PathBuf>, PickerUnavailable>;
    /// The operating system's save dialog with `file_name` proposed. `None` when the person
    /// cancelled.
    async fn save_path(&self, file_name: &str) -> Option<PathBuf>;
    /// Runs step 10 (or step 15) for `trigger`; returns at once, the shutdown runs on its own task.
    fn quit(&self, trigger: QuitTrigger);
    /// Emits `ordin://appearance` to the main window.
    fn appearance_changed(&self, appearance: Appearance);
}

/// What step 10 does with one quit trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitDecision {
    /// 10.1: a shutdown is already running; nothing more.
    AlreadyRunning,
    /// 10.2: emit `ordin://quit-requested { running_sessions }` and stop here.
    Ask { running_sessions: u32 },
    /// 10.3: run `Core::shutdown` with this reason.
    Proceed(QuitReason),
}

/// The state of step 10 and step 15 across triggers: whether a shutdown runs, and whether an update
/// restart was asked for and waits for the person's answer to the quit question (DSK-UP-13).
#[derive(Default)]
pub struct QuitControl {
    state: Mutex<QuitState>,
}

#[derive(Default)]
struct QuitState {
    running: bool,
    update_requested: bool,
}

impl QuitControl {
    fn state(&self) -> std::sync::MutexGuard<'_, QuitState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Step 10.1, 10.2 and 10.5, and step 15.1: the decision for `trigger` while
    /// `running_sessions` local sessions run. A `Proceed` marks the shutdown as running.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-13
    pub fn decide(&self, trigger: QuitTrigger, running_sessions: u32) -> QuitDecision {
        let mut state = self.state();
        if state.running {
            return QuitDecision::AlreadyRunning;
        }
        let reason = match trigger {
            QuitTrigger::OperatingSystemEnding => QuitReason::OperatingSystemEnding,
            // spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
            QuitTrigger::HarnessChosen => QuitReason::HarnessChosen,
            QuitTrigger::Update => {
                state.update_requested = true;
                if running_sessions > 0 {
                    return QuitDecision::Ask { running_sessions };
                }
                QuitReason::Update
            }
            // The screens' answer to the quit question: it continues an update restart asked for.
            QuitTrigger::AppQuit if state.update_requested => QuitReason::Update,
            QuitTrigger::AppQuit => QuitReason::PersonQuit,
            QuitTrigger::WindowClosed | QuitTrigger::Menu => {
                // A plain quit is never turned into an update restart.
                state.update_requested = false;
                if running_sessions > 0 {
                    return QuitDecision::Ask { running_sessions };
                }
                QuitReason::PersonQuit
            }
        };
        state.update_requested = false;
        state.running = true;
        QuitDecision::Proceed(reason)
    }

    /// The main window gained focus with no `app.quit` since the quit question: a declined update
    /// restart is forgotten (DSK-UP-13 step 1).
    pub fn window_focused(&self) {
        self.state().update_requested = false;
    }

    /// Whether a shutdown runs.
    pub fn running(&self) -> bool {
        self.state().running
    }
}

/// `app.open_web_page` (DSK-WEB-1): the fixed address of each page ordin names.
// spec: ordin-desktop/components/OrdinDesktop#DSK-WEB-1
pub fn web_page_address(page: ordin_service_api::commands::WebPage) -> &'static str {
    use ordin_service_api::commands::WebPage;
    match page {
        WebPage::TelegramBotfather => "https://t.me/BotFather",
        WebPage::AtlassianApiTokens => {
            "https://id.atlassian.com/manage-profile/security/api-tokens"
        }
        WebPage::ClaudeCodeInstall => "https://claude.com/claude-code",
        WebPage::GithubTokens => "https://github.com/settings/tokens",
        WebPage::GithubSshKeys => "https://github.com/settings/ssh/new",
        // spec: ordin-desktop/components/ServiceApi#SVC-MIN-4
        WebPage::ObsDownload => "https://obsproject.com/download",
    }
}

/// `app.open_slack_link`: at most 2,048 bytes and matching
/// `^https://[a-z0-9-]{1,80}\.slack\.com/archives/[CG][A-Z0-9]{8,20}/p[0-9]{16}(\?.*)?$`.
pub fn is_slack_permalink(url: &str) -> bool {
    if url.len() > 2048 {
        return false;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let Some((host, rest)) = rest.split_once(".slack.com/archives/") else {
        return false;
    };
    let host_ok = (1..=80).contains(&host.len())
        && host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !host_ok {
        return false;
    }
    let (path, _query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };
    let Some((channel, message)) = path.split_once("/p") else {
        return false;
    };
    let channel_ok = (9..=21).contains(&channel.len())
        && matches!(channel.as_bytes()[0], b'C' | b'G')
        && channel[1..]
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    let message_ok = message.len() == 16 && message.bytes().all(|b| b.is_ascii_digit());
    channel_ok && message_ok
}

/// `files.save_export`'s `file_name`: 1–255 characters with no `/`, `\` or NUL.
pub fn export_name_valid(file_name: &str) -> bool {
    let count = file_name.chars().count();
    (1..=255).contains(&count) && !file_name.contains(['/', '\\', '\0'])
}

/// The longest computer name a pairing carries (RemoteApi.md `PairRequest.computer_name`).
const COMPUTER_NAME_MAX_CHARS: usize = 63;

/// DSK-REC-34: the operating system's device name with control characters removed, spaces trimmed
/// at both ends and cut to 63 characters at a character boundary; `unknown` when nothing is left.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-34
pub fn computer_name(device_name: &str) -> String {
    let cleaned: String = device_name.chars().filter(|c| !c.is_control()).collect();
    let cut: String = cleaned
        .trim()
        .chars()
        .take(COMPUTER_NAME_MAX_CHARS)
        .collect();
    let name = cut.trim_end();
    if name.is_empty() {
        "unknown".to_owned()
    } else {
        name.to_owned()
    }
}

/// Writes `bytes` atomically to `path`: a temporary file in the same folder, then a rename. A
/// failure leaves no temporary file.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let folder = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no folder"))?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let tmp = folder.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::now_v7()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    // Scenario: Edge case — an empty device name (DSK-REC-34)
    #[test]
    fn an_empty_device_name_is_unknown() {
        assert_eq!(computer_name(""), "unknown");
        assert_eq!(computer_name("  \u{7}\t "), "unknown");
    }

    // Scenario: Edge case — a long device name is cut for pairing (DSK-REC-34)
    #[test]
    fn a_long_device_name_is_cut_at_a_character_boundary() {
        let long = "é".repeat(80);
        let name = computer_name(&long);
        assert_eq!(name.chars().count(), 63);
        assert_eq!(name, "é".repeat(63));
        assert_eq!(computer_name("  Asha's\nlaptop  "), "Asha'slaptop");
    }

    #[test]
    fn quit_with_nothing_running_does_not_ask() {
        let control = QuitControl::default();
        assert_eq!(
            control.decide(QuitTrigger::Menu, 0),
            QuitDecision::Proceed(QuitReason::PersonQuit)
        );
        assert!(control.running());
        // 10.1: a second trigger does nothing more.
        assert_eq!(
            control.decide(QuitTrigger::AppQuit, 0),
            QuitDecision::AlreadyRunning
        );
    }

    // Gherkin: Edge case — quit with running sessions asks first.
    #[test]
    fn quit_with_running_sessions_asks_first_and_app_quit_proceeds() {
        let control = QuitControl::default();
        assert_eq!(
            control.decide(QuitTrigger::WindowClosed, 2),
            QuitDecision::Ask {
                running_sessions: 2
            }
        );
        assert!(!control.running());
        assert_eq!(
            control.decide(QuitTrigger::AppQuit, 2),
            QuitDecision::Proceed(QuitReason::PersonQuit)
        );
    }

    // DSK-CHO-2 step 3: the first-start answer restarts without asking.
    #[test]
    fn a_harness_chosen_at_first_start_proceeds_without_asking() {
        let control = QuitControl::default();
        assert_eq!(
            control.decide(QuitTrigger::HarnessChosen, 0),
            QuitDecision::Proceed(QuitReason::HarnessChosen)
        );
        assert_eq!(QuitReason::HarnessChosen.log_name(), "harness_chosen");
    }

    #[test]
    fn the_operating_system_ending_never_asks() {
        let control = QuitControl::default();
        assert_eq!(
            control.decide(QuitTrigger::OperatingSystemEnding, 3),
            QuitDecision::Proceed(QuitReason::OperatingSystemEnding)
        );
    }

    // Gherkin (ADR-114): Happy path — restart to update; Edge case — restart to update while
    // sessions run.
    #[test]
    fn an_update_restart_asks_when_sessions_run_and_app_quit_continues_it() {
        let control = QuitControl::default();
        assert_eq!(
            control.decide(QuitTrigger::Update, 2),
            QuitDecision::Ask {
                running_sessions: 2
            }
        );
        assert_eq!(
            control.decide(QuitTrigger::AppQuit, 2),
            QuitDecision::Proceed(QuitReason::Update)
        );
        let control = QuitControl::default();
        assert_eq!(
            control.decide(QuitTrigger::Update, 0),
            QuitDecision::Proceed(QuitReason::Update)
        );
    }

    // Gherkin (ADR-114): Edge case — a declined update restart does not turn a later quit into an
    // update.
    #[test]
    fn a_declined_update_restart_is_forgotten_on_focus() {
        let control = QuitControl::default();
        assert!(matches!(
            control.decide(QuitTrigger::Update, 1),
            QuitDecision::Ask { .. }
        ));
        control.window_focused();
        assert_eq!(
            control.decide(QuitTrigger::AppQuit, 0),
            QuitDecision::Proceed(QuitReason::PersonQuit)
        );
        let control = QuitControl::default();
        assert!(matches!(
            control.decide(QuitTrigger::Update, 1),
            QuitDecision::Ask { .. }
        ));
        assert_eq!(
            control.decide(QuitTrigger::Menu, 0),
            QuitDecision::Proceed(QuitReason::PersonQuit)
        );
    }

    #[test]
    fn slack_permalinks_are_checked() {
        let good = [
            "https://acme.slack.com/archives/C01234567/p1700000000123456",
            "https://acme-eu.slack.com/archives/G0123456789ABCDEFGHI/p1700000000123456?thread_ts=1.2",
        ];
        for url in good {
            assert!(is_slack_permalink(url), "{url}");
        }
        let long_query = format!(
            "https://acme.slack.com/archives/C01234567/p1700000000123456?{}",
            "a".repeat(2048)
        );
        let bad = [
            "http://acme.slack.com/archives/C01234567/p1700000000123456",
            "https://Acme.slack.com/archives/C01234567/p1700000000123456",
            "https://acme.slack.com.evil.example/archives/C01234567/p1700000000123456",
            "https://evil.example/x.slack.com/archives/C01234567/p1700000000123456",
            "https://acme.slack.com/archives/D01234567/p1700000000123456",
            "https://acme.slack.com/archives/C0123/p1700000000123456",
            "https://acme.slack.com/archives/C01234567/p170000000012345",
            "https://acme.slack.com/archives/C01234567/p1700000000123456/extra",
            "https://.slack.com/archives/C01234567/p1700000000123456",
            long_query.as_str(),
        ];
        for url in bad {
            assert!(!is_slack_permalink(url), "{url}");
        }
    }

    #[test]
    fn export_names_are_checked() {
        assert!(export_name_valid("workflow.yaml"));
        assert!(export_name_valid(&"é".repeat(255)));
        for bad in ["", "a/b", "a\\b", "a\0b"] {
            assert!(!export_name_valid(bad), "{bad:?}");
        }
        assert!(!export_name_valid(&"a".repeat(256)));
    }

    #[test]
    fn an_atomic_write_leaves_only_the_file() {
        let dir = TempDir::new("atomic");
        let path = dir.path().join("out.txt");
        write_atomically(&path, b"one").expect("write");
        write_atomically(&path, b"two").expect("replace");
        assert_eq!(std::fs::read(&path).expect("file"), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 1);
        // A folder that does not exist: the write fails and leaves nothing.
        let err = write_atomically(&dir.path().join("missing/out.txt"), b"x");
        assert!(err.is_err());
    }
}
