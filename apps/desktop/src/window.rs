//! The Tauri shell and the main window (OrdinDesktop.md *Internal Logic* steps 1, 2, 5, 6, 9, 10
//! and 15). Built for macOS and Windows only; the core it starts is the library's [`Core`].
//
// `installation_id` is a new UUIDv7 at each start, written nowhere (DSK-REC-33); the computer's name
// is the operating system's device name, cleaned (DSK-REC-34); the bundled catalogue is the
// `catalogue` folder of Tauri's resource directory, a pinned commit frozen in at release, and the
// installer values are compiled in (DSK-REC-39, DSK-REC-39a); on macOS the application menu's Quit is
// the program's own item, so it asks first (DSK-REC-35).

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ordin_command_bridge::CommandBridge;
use ordin_types::CodedError;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::core::{log_start_failed, AfterQuit, Core};
use crate::error::{StartError, START_FAILED};
use crate::notifier::{self, Notifier};
use crate::paths::DesktopPaths;
use crate::prefs::Appearance;
use crate::recovery::{self, DataOffer};
use crate::shell::{PickerUnavailable, QuitDecision, QuitTrigger, Shell};
use crate::updater::TauriUpdater;
use crate::wiring::CoreInputs;

/// The main window's label.
pub const MAIN: &str = "main";

/// The id of the macOS application menu's Quit item the program builds (DSK-REC-35).
#[cfg(target_os = "macos")]
const QUIT_MENU_ID: &str = "desktop-menu-quit";

/// Step 9's message box text, from the en-GB words this program carries when Localisation is not
/// constructed. It says what the person can do themselves (amended January 2026, user decision).
const START_FAILED_TEXT: &str =
    "ordin couldn't start on this computer. Open it again; if this happens again, reinstall ordin.";

/// The message box text when no `claude` program is found, in the same fallback words as
/// [`START_FAILED_TEXT`]. The person installs Claude Code themselves; nobody else is involved.
const CLAUDE_MISSING_TEXT: &str = "ordin needs Claude Code, and it is not installed on this computer. Install Claude Code first, then open ordin again.";

/// The window shell over Tauri: the core is set once step 4 has run.
pub struct TauriShell {
    app: AppHandle,
    core: OnceLock<Arc<Core>>,
}

impl TauriShell {
    fn main_window(&self) -> Option<tauri::WebviewWindow> {
        self.app.get_webview_window(MAIN)
    }

    /// A journeys test build only: the answer the journey runner left for the next system dialog, in
    /// `<data root>/journeys-dialog-answer.txt` (one path), read once and deleted. `None` opens the real
    /// dialog. Never in a release (DSK-JRN-2).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-JRN-3
    #[cfg(feature = "journeys")]
    fn journeys_answer(&self) -> Option<PathBuf> {
        let file = self
            .app
            .path()
            .app_local_data_dir()
            .ok()?
            .join("journeys-dialog-answer.txt");
        let text = std::fs::read_to_string(&file).ok()?;
        let _ = std::fs::remove_file(&file);
        let answer = text.trim();
        (!answer.is_empty()).then(|| PathBuf::from(answer))
    }

    #[cfg(not(feature = "journeys"))]
    fn journeys_answer(&self) -> Option<PathBuf> {
        None
    }
}

#[async_trait::async_trait]
impl Shell for TauriShell {
    async fn pick_folder(&self) -> Result<Option<PathBuf>, PickerUnavailable> {
        if let Some(answer) = self.journeys_answer() {
            return Ok(Some(answer));
        }
        let mut dialog = self.app.dialog().file();
        if let Some(window) = self.main_window() {
            dialog = dialog.set_parent(&window);
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        dialog.pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
        let picked = rx.await.map_err(|_| PickerUnavailable)?;
        picked
            .map(|path| path.into_path().map_err(|_| PickerUnavailable))
            .transpose()
    }

    async fn pick_file(&self, title: &str) -> Result<Option<PathBuf>, PickerUnavailable> {
        if let Some(answer) = self.journeys_answer() {
            return Ok(Some(answer));
        }
        let mut dialog = self.app.dialog().file().set_title(title);
        if let Some(window) = self.main_window() {
            dialog = dialog.set_parent(&window);
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        dialog.pick_file(move |picked| {
            let _ = tx.send(picked);
        });
        let picked = rx.await.map_err(|_| PickerUnavailable)?;
        picked
            .map(|path| path.into_path().map_err(|_| PickerUnavailable))
            .transpose()
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-2
    async fn pick_file_of(
        &self,
        title: &str,
        filter_name: &str,
        extensions: &[&str],
    ) -> Result<Option<PathBuf>, PickerUnavailable> {
        if let Some(answer) = self.journeys_answer() {
            return Ok(Some(answer));
        }
        let mut dialog = self
            .app
            .dialog()
            .file()
            .set_title(title)
            .add_filter(filter_name, extensions);
        if let Some(window) = self.main_window() {
            dialog = dialog.set_parent(&window);
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        dialog.pick_file(move |picked| {
            let _ = tx.send(picked);
        });
        let picked = rx.await.map_err(|_| PickerUnavailable)?;
        picked
            .map(|path| path.into_path().map_err(|_| PickerUnavailable))
            .transpose()
    }

    async fn save_path(&self, file_name: &str) -> Option<PathBuf> {
        if let Some(answer) = self.journeys_answer() {
            return Some(answer);
        }
        let mut dialog = self.app.dialog().file().set_file_name(file_name);
        if let Some(window) = self.main_window() {
            dialog = dialog.set_parent(&window);
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        dialog.save_file(move |chosen| {
            let _ = tx.send(chosen);
        });
        rx.await
            .ok()
            .flatten()
            .and_then(|path| path.into_path().ok())
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-13
    fn quit(&self, trigger: QuitTrigger) {
        let Some(core) = self.core.get().cloned() else {
            return;
        };
        match core.quit.decide(trigger, core.running_local_sessions()) {
            QuitDecision::AlreadyRunning => {}
            QuitDecision::Ask { running_sessions } => {
                let _ = self.app.emit_to(
                    MAIN,
                    "ordin://quit-requested",
                    serde_json::json!({ "running_sessions": running_sessions }),
                );
            }
            QuitDecision::Proceed(reason) => {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move {
                    match core.quit(reason).await {
                        AfterQuit::Exit => app.exit(0),
                        AfterQuit::Restart => app.restart(),
                    }
                });
            }
        }
    }

    fn appearance_changed(&self, appearance: Appearance) {
        let _ = self.app.emit_to(
            MAIN,
            "ordin://appearance",
            serde_json::json!({ "appearance": appearance }),
        );
    }
}

/// `UrlOpener` and `BrowserOpener` over `tauri-plugin-opener`.
struct Opener(AppHandle);

impl ordin_setup::UrlOpener for Opener {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    }
}

impl ordin_identity::BrowserOpener for Opener {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    }
}

/// Step 2: the JSON log through `ordin.<date>.log` files — a new one each day and at 20 MB, at most
/// 14 kept and 100 MB in all (DSK-LOG-1, DSK-LOG-2) — level `info`, and the panic hook. The writer's
/// guard lives for the process. `tracing-appender` rolls by time only, so the program's own
/// [`crate::log_files::LogFiles`] is the file writer, behind `tracing-appender`'s non-blocking
/// thread.
// spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-1
fn install_logging(logs: &std::path::Path) {
    let Ok((files, not_deleted)) =
        crate::log_files::LogFiles::open(logs, crate::log_files::Limits::DESKTOP)
    else {
        return;
    };
    crate::log_files::set_installed(Some(files.clone()));
    let (subscriber, guard) = crate::log_files::subscriber(files);
    std::mem::forget(guard);
    // `try_init` also routes the `log` crate's records into `tracing`, where they are libraries'
    // lines (Logging.md R-5).
    let _ = tracing_subscriber::util::SubscriberInitExt::try_init(subscriber);
    for file in not_deleted {
        tracing::warn!(
            component = "desktop",
            code = crate::error::LOG_FILE_NOT_DELETED,
            file = file.as_str(),
            "log_file_not_deleted"
        );
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // The location only: a panic's message can quote the text being handled (Logging.md L-1).
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        tracing::error!(component = "desktop", location = location.as_str(), "panic");
        default_hook(info);
    }));
}

/// The first `claude` on `PATH`, else in the person's own folders a Finder or Dock launch leaves out
/// of `PATH`, by the Setup Assistant's own rule and folder list (SetupAssistant.md SET-R1-7), else the
/// bare name (OQ-REC-4 option (b), the code's current reading).
// spec: ordin-desktop/components/SetupAssistant#SET-R1-7
fn claude_on_path() -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|home| !home.is_empty())
        .map(PathBuf::from);
    ordin_setup::find_claude_code(std::env::var_os("PATH").as_deref(), home.as_deref())
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// The build switches of this build, for the `start` line.
fn features() -> Vec<&'static str> {
    [
        ("authoring", cfg!(feature = "authoring")),
        ("copilot", cfg!(feature = "copilot")),
        ("dictation", cfg!(feature = "dictation")),
        ("handover", cfg!(feature = "handover")),
        ("hosted-agents", cfg!(feature = "hosted-agents")),
        ("journeys", cfg!(feature = "journeys")),
        ("knowledge-agents", cfg!(feature = "knowledge-agents")),
        ("mail", cfg!(feature = "mail")),
        ("minutes", cfg!(feature = "minutes")),
        ("sandbox", cfg!(feature = "sandbox")),
        ("slack", cfg!(feature = "slack")),
        ("team-knowledge", cfg!(feature = "team-knowledge")),
        ("telegram", cfg!(feature = "telegram")),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect()
}

fn module_failed() -> StartError {
    StartError::ModuleFailed {
        module: "ordin-desktop",
        code: START_FAILED.to_owned(),
    }
}

/// Step 2 and the inputs of step 3.
fn inputs(app: &AppHandle, shell: Arc<TauriShell>) -> Result<CoreInputs, StartError> {
    let path = app.path();
    let data_root = path
        .app_local_data_dir()
        .map_err(|e| StartError::DataRootUnwritable {
            path: PathBuf::new(),
            source: std::io::Error::other(e.to_string()),
        })?;
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_default();
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-33
    let installation_id = uuid::Uuid::now_v7();
    let paths = DesktopPaths::new(data_root, &exe_dir, &installation_id.to_string());
    paths.prepare()?;
    install_logging(&paths.logs);
    tracing::info!(
        component = "desktop",
        features = features().join(",").as_str(),
        "start"
    );
    let resources = path.resource_dir().unwrap_or_else(|_| exe_dir.clone());
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-2
    let models = crate::memory_models::models_input(
        option_env!("ORDIN_MEMORY_MODELS_URL"),
        option_env!("ORDIN_MEMORY_MODELS_SHA256"),
        option_env!("ORDIN_MEMORY_MODELS_BYTES"),
        option_env!("ORDIN_MEMORY_MODELS_MANIFEST"),
    );
    let home = path.home_dir().unwrap_or_default();
    let opener = Arc::new(Opener(app.clone()));
    Ok(CoreInputs {
        http: crate::http::outbound_client().map_err(|_| module_failed())?,
        bundle_identifier: app.config().identifier.clone(),
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-34
        computer_name: crate::shell::computer_name(&whoami::devicename().unwrap_or_default()),
        installation_id,
        os_locales: sys_locale::get_locales().collect(),
        documents_dir: path.document_dir().unwrap_or_else(|_| home.clone()),
        home,
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-46
        installer_defaults: crate::wiring::installer_defaults_from(option_env!(
            "ORDIN_UPDATE_FEED_URL"
        )),
        browser: Some(opener.clone()),
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-1
        models_dir: paths.data_root.join("models"),
        model_manifest: models.manifest,
        models_source: models.source,
        claude_path: claude_on_path(),
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-39
        bundled_catalogue: resources.join("catalogue"),
        url_opener: Some(opener),
        updater: Some(Arc::new(TauriUpdater::new(app.clone()))),
        shell: Some(shell),
        paths,
    })
}

/// Step 9: the `start_failed` line and the message box; its button exits with status 1.
fn start_failed(app: &AppHandle, error: &StartError) {
    let reference = uuid::Uuid::now_v7();
    log_start_failed(error);
    tracing::error!(
        component = "desktop",
        reference = reference.to_string().as_str(),
        "start_failed_reference"
    );
    app.dialog()
        .message(format!("{START_FAILED_TEXT}\n\nReference {reference}"))
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| std::process::exit(1));
}

/// DSK-DATA-1: the saved data does not match this program; the person chooses what happens to it.
// spec: ordin-desktop/components/OrdinDesktop#DSK-DATA-1
fn data_offer(app: &AppHandle, error: &StartError, offer: DataOffer, data_root: PathBuf) {
    let reference = uuid::Uuid::now_v7();
    log_start_failed(error);
    tracing::error!(
        component = "desktop",
        reference = reference.to_string().as_str(),
        "start_failed_reference"
    );
    let buttons = match offer {
        DataOffer::Mismatch => MessageDialogButtons::YesNoCancelCustom(
            recovery::KEEP_DATA.to_owned(),
            recovery::EMPTY_DATA.to_owned(),
            recovery::QUIT.to_owned(),
        ),
        DataOffer::Newer => MessageDialogButtons::OkCancelCustom(
            recovery::EMPTY_DATA.to_owned(),
            recovery::QUIT.to_owned(),
        ),
    };
    let handle = app.clone();
    app.dialog()
        .message(format!("{}\n\nReference {reference}", offer.text()))
        .kind(MessageDialogKind::Warning)
        .buttons(buttons)
        .show_with_result(move |choice| {
            // Custom buttons answer with their label; a platform that answers with the button's
            // role is read by position.
            let chosen = match (choice, offer) {
                (MessageDialogResult::Custom(label), _) => label,
                (MessageDialogResult::Yes, DataOffer::Mismatch) => recovery::KEEP_DATA.to_owned(),
                (MessageDialogResult::No, DataOffer::Mismatch)
                | (MessageDialogResult::Ok, DataOffer::Newer) => recovery::EMPTY_DATA.to_owned(),
                _ => recovery::QUIT.to_owned(),
            };
            match chosen.as_str() {
                recovery::KEEP_DATA => {
                    match tauri::async_runtime::block_on(recovery::keep_data(&data_root)) {
                        Ok(_) => handle.restart(),
                        Err(e) => start_failed(
                            &handle,
                            &StartError::ModuleFailed {
                                module: crate::plan::Row::Journal.module(),
                                code: e.code().to_owned(),
                            },
                        ),
                    }
                }
                recovery::EMPTY_DATA => match recovery::set_aside(&data_root) {
                    Ok(_) => handle.restart(),
                    Err(e) => start_failed(&handle, &e),
                },
                _ => std::process::exit(1),
            }
        });
}

/// Tells the person Claude Code has to be installed first; its button exits with status 1.
fn claude_missing(app: &AppHandle) {
    tracing::error!(component = "desktop", "claude_not_found");
    app.dialog()
        .message(CLAUDE_MISSING_TEXT)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::Ok)
        .show(|_| std::process::exit(1));
}

/// Steps 2 to 6, inside Tauri's `setup`.
fn setup(app: &AppHandle, shell: Arc<TauriShell>, process_start: Instant) {
    let inputs = match inputs(app, shell.clone()) {
        Ok(inputs) => inputs,
        Err(error) => return start_failed(app, &error),
    };
    // `claude_on_path` answers the bare name when it finds nothing, which is not an absolute path.
    if !inputs.claude_path.is_absolute() {
        return claude_missing(app);
    }
    let data_root = inputs.paths.data_root.clone();
    let preferences = inputs.paths.preferences.clone();
    let core = match tauri::async_runtime::block_on(Core::start(inputs)) {
        Ok(core) => core,
        Err(error) => match DataOffer::for_error(&error) {
            Some(offer) => return data_offer(app, &error, offer, data_root),
            None => return start_failed(app, &error),
        },
    };
    // The plugin's commands find the bridge in managed state, which exists before the window.
    if let Some(bridge) = core.modules.bridge.clone() {
        app.manage::<CommandBridge>((*bridge).clone());
    }
    let title = core
        .modules
        .brand
        .as_ref()
        .map(|b| b.strings().product_name)
        .unwrap_or_else(|| "ordin".to_owned());
    #[cfg(target_os = "macos")]
    {
        let label = match &core.modules.localisation {
            Some(localisation) => localisation.message(
                ordin_localisation::id::DESKTOP_MENU_QUIT,
                &ordin_localisation::MessageArgs::new()
                    .with("product", ordin_localisation::ArgValue::Text(title.clone())),
            ),
            None => format!("Quit {title}"),
        };
        if quit_asks_first(app, &label).is_err() {
            tracing::warn!(component = "desktop", "menu_quit_not_built");
        }
    }
    // Step 7: the computer's notifications, from the bridge's merged stream (DSK-NTF-1).
    spawn_notifier(app, &core, preferences);
    let _ = shell.core.set(core);
    // Step 6.
    let builder = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(1280.0, 800.0)
        .min_inner_size(480.0, 560.0)
        .focused(true);
    // A journeys test build only: the window shell's default browser arguments plus WebView2's debugging
    // port on the loopback address, for Playwright. Never in a release (DSK-JRN-2).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-JRN-1
    #[cfg(all(feature = "journeys", target_os = "windows"))]
    let builder = builder.additional_browser_args(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --remote-debugging-port=9222",
    );
    let window = builder.build();
    match window {
        Ok(_) => tracing::info!(
            component = "desktop",
            ms = u64::try_from(process_start.elapsed().as_millis()).unwrap_or(u64::MAX),
            "window_shown"
        ),
        Err(_) => start_failed(app, &module_failed()),
    }
}

/// Step 7 (DSK-NTF-1): reads every event the Command Bridge delivers and shows the notice
/// [`Notifier`] maps it to, when its Settings › Notifications switch is on and the main window is not
/// in front. An approval is held for [`notifier::APPROVAL_HOLD_MS`] and dropped when it was decided
/// meanwhile. A refusal by the operating system is logged as `DSK_NOTIFY_FAILED` and not retried.
// spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
fn spawn_notifier(app: &AppHandle, core: &Arc<Core>, preferences: PathBuf) {
    let (Some(bridge), Some(local)) = (core.modules.bridge.clone(), core.modules.backend) else {
        return;
    };
    let mut events = bridge.subscribe_all();
    let notifier = NOTIFIER
        .get_or_init(|| Arc::new(Notifier::new(local)))
        .clone();
    let preferences = Arc::new(crate::prefs::Preferences::new(preferences));
    let app = app.clone();
    let core = core.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let env = match events.recv().await {
                Ok(env) => env,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let connector = core.modules.connector.clone();
            let Some(notice) = notifier.notice(&env, |backend| {
                connector.as_ref().and_then(|c| c.display_name(backend))
            }) else {
                continue;
            };
            let (app, core, notifier, preferences) = (
                app.clone(),
                core.clone(),
                notifier.clone(),
                preferences.clone(),
            );
            tauri::async_runtime::spawn(async move {
                if notice.approval.is_some() {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        notifier::APPROVAL_HOLD_MS,
                    ))
                    .await;
                    if !notifier.still_pending(&notice) {
                        return;
                    }
                }
                let prefs = preferences.read().notifications;
                if !notifier::should_show(notice.kind, &prefs, window_in_front(&app)) {
                    return;
                }
                let (title, body) = notice_text(&core, &notice);
                let shown = app
                    .notification()
                    .builder()
                    .title(title)
                    .body(body)
                    .show()
                    .is_ok();
                if shown {
                    notifier.shown(&notice);
                } else {
                    tracing::warn!(
                        component = "desktop",
                        code = crate::error::NOTIFY_FAILED,
                        kind = notice.kind.log_name(),
                        "notification_failed"
                    );
                }
                tracing::info!(
                    component = "desktop",
                    kind = notice.kind.log_name(),
                    shown,
                    "notification"
                );
            });
        }
    });
}

/// The notifier of this run, for the focus that opens the last notice's item (step 7.5).
static NOTIFIER: OnceLock<Arc<Notifier>> = OnceLock::new();

/// Step 7.5 (DSK-NTF-1): the main window gained focus, as clicking a notice does; the last notice's
/// item is opened by the screens when it was shown within five minutes.
// spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
fn open_notice_target(app: &AppHandle) {
    let Some(target) = NOTIFIER.get().and_then(|n| n.take_on_focus()) else {
        return;
    };
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if app.emit("ordin://open-target", target).is_err() {
        tracing::warn!(component = "desktop", "open_target_not_sent");
    }
}

/// The main window is focused, visible and not minimised: the person is already looking at it.
fn window_in_front(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN).is_some_and(|w| {
        w.is_focused().unwrap_or(false)
            && w.is_visible().unwrap_or(false)
            && !w.is_minimized().unwrap_or(false)
    })
}

/// A notice's title and body in the person's language.
fn notice_text(core: &Core, notice: &notifier::Notice) -> (String, String) {
    use ordin_localisation::{id, ArgValue, MessageArgs};
    let Some(words) = &core.modules.localisation else {
        return (String::new(), String::new());
    };
    let coordinator = core
        .modules
        .brand
        .as_ref()
        .map(|b| b.strings().coordinator_name)
        .unwrap_or_else(|| "Ordin".to_owned());
    let text = |message, args: MessageArgs| words.message(message, &args);
    let (title, body) = match &notice.words {
        notifier::Words::Approval { question } => (
            text(
                id::NOTIFICATION_APPROVAL_TITLE,
                MessageArgs::new()
                    .with("coordinator", ArgValue::Text(coordinator))
                    .with("question", ArgValue::Text(question.clone())),
            ),
            text(id::NOTIFICATION_APPROVAL_BODY, MessageArgs::new()),
        ),
        notifier::Words::RunDone => (
            text(id::NOTIFICATION_RUN_DONE_TITLE_UNNAMED, MessageArgs::new()),
            text(id::NOTIFICATION_DONE_BODY, MessageArgs::new()),
        ),
        notifier::Words::CrewDone => (
            text(id::NOTIFICATION_CREW_DONE_TITLE, MessageArgs::new()),
            text(id::NOTIFICATION_DONE_BODY, MessageArgs::new()),
        ),
        notifier::Words::Question { question } => (
            text(
                id::NOTIFICATION_QUESTION_TITLE,
                MessageArgs::new().with("coordinator", ArgValue::Text(coordinator)),
            ),
            question.clone(),
        ),
    };
    let body = match &notice.server {
        Some(server) => text(
            id::NOTIFICATION_ON_SERVER,
            MessageArgs::new()
                .with("text", ArgValue::Text(body))
                .with("server", ArgValue::Text(server.clone())),
        ),
        None => body,
    };
    (title, body)
}

/// DSK-REC-35: the default application menu with its predefined Quit replaced by the program's own
/// item, labelled `desktop-menu-quit` with ⌘Q; choosing it runs step 10 with the trigger *menu Quit*.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-35
#[cfg(target_os = "macos")]
fn quit_asks_first(app: &AppHandle, label: &str) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind};
    let menu = Menu::default(app)?;
    if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
        let items = app_menu.items()?;
        if let Some(MenuItemKind::Predefined(quit)) = items.last() {
            app_menu.remove(quit)?;
        }
        let item = MenuItem::with_id(app, QUIT_MENU_ID, label, true, Some("CmdOrCtrl+Q"))?;
        app_menu.append(&item)?;
    }
    app.set_menu(menu)?;
    Ok(())
}

/// `main`: the single-instance plug-in first (step 1), then the shell plug-ins, the Command
/// Bridge's plug-in, and the core started in `setup`.
pub fn run() {
    let process_start = Instant::now();
    let shell_slot: Arc<OnceLock<Arc<TauriShell>>> = Arc::new(OnceLock::new());
    let built = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window(MAIN) {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            tracing::info!(component = "desktop", "single_instance_focus");
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(CommandBridge::plugin())
        .on_menu_event({
            let shell_slot = shell_slot.clone();
            move |_app, event| {
                #[cfg(target_os = "macos")]
                if event.id().0 == QUIT_MENU_ID {
                    if let Some(shell) = shell_slot.get() {
                        shell.quit(QuitTrigger::Menu);
                    }
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (&shell_slot, event);
            }
        })
        .setup({
            let shell_slot = shell_slot.clone();
            move |app| {
                let shell = Arc::new(TauriShell {
                    app: app.handle().clone(),
                    core: OnceLock::new(),
                });
                let _ = shell_slot.set(shell.clone());
                setup(app.handle(), shell, process_start);
                Ok(())
            }
        })
        .build(tauri::generate_context!());
    let app = match built {
        Ok(app) => app,
        Err(error) => {
            eprintln!("ordin couldn't start: {error}");
            std::process::exit(1);
        }
    };
    app.run(move |_app, event| {
        let Some(shell) = shell_slot.get() else {
            return;
        };
        match event {
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } if label == MAIN => {
                api.prevent_close();
                shell.quit(QuitTrigger::WindowClosed);
            }
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::Focused(true),
                ..
            } if label == MAIN => {
                if let Some(core) = shell.core.get() {
                    core.quit.window_focused();
                }
                open_notice_target(_app);
            }
            // Step 10.5: with the predefined Quit gone (DSK-REC-35), an exit the program did not ask
            // for is the operating system ending the session.
            RunEvent::ExitRequested {
                api, code: None, ..
            } if shell.core.get().is_some_and(|core| !core.quit.running()) => {
                api.prevent_exit();
                shell.quit(QuitTrigger::OperatingSystemEnding);
            }
            _ => {}
        }
    });
}
