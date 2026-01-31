//! The Command Bridge (CommandBridge.md): the Service API's transport on Tauri's IPC, inside the
//! desktop program. It receives every command the screens send, refuses anything that is not a
//! well-formed command of the catalogue, and routes it: a command for the desktop's own backend goes to
//! the program's `ServiceApi` through `ordin_service_api::handle`; a command for a paired server's
//! backend goes to the Server Connector. It delivers every event the screens need, in order per
//! backend, in batches over one Tauri channel per window, and carries the unjournalled streams
//! (dictation, terminal, language server) to the main window. The WebView makes no network call and
//! holds no key; the Bridge is the only door from the WebView into the core.
//!
//! The Bridge owns no table. Its replay buffers and window queues are memory only.
//!
//! Conditional assertion relied on (CommandBridge.md *Constants and rules*): C-73, *Tauri's IPC can
//! carry the Service API's commands and events at the desktop's targets with the same JSON types the
//! Remote API uses*. The design meets it with batched channel messages; `tests/performance.rs` holds
//! the spike (1,000 events a second for one minute).
#![forbid(unsafe_code)]

mod bridge;
mod error;
mod events;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod ipc;
mod log;
mod route;
mod streams;

pub use bridge::{BridgeConfig, CommandBridge, DEFAULT_SERVER_REPLAY_EVENTS};
pub use error::{BridgeError, StreamSendError, ALL_CODES};
pub use events::{EventBatch, EventOut, EventsOpened};
pub use streams::{ScreenStreams, StreamKind, StreamMessage, StreamOut};

macro_rules! ipc_names {
    ($( ($module:ident, $verb:ident, $method:ident, $input:ty, $result:ty, $scope:ident, $effect:ident); )*) => {
        /// The IPC names of every catalogue command plus `service_events` and `service_streams`, for
        /// the desktop program's build script:
        /// `tauri_build::Attributes::new().plugin("ordin", InlinedPlugin::new().commands(IPC_COMMANDS)
        ///     .default_permission(DefaultPermissionRule::AllowAllCommands))`, so that the permission
        /// set `ordin:default` allows every one of them.
        // spec: ordin-desktop/components/CommandBridge#SA-RECON-02
        pub const IPC_COMMANDS: &[&str] = &[
            $( stringify!($method), )*
            "service_events",
            "service_streams",
        ];
    };
}

ordin_service_api::for_each_command!(ipc_names);

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl CommandBridge {
    /// The Tauri plugin carrying every generated command, `service_events` and `service_streams`.
    pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
        ipc::build()
    }
}
