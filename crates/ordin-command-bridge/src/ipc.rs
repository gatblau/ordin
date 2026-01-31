//! The Tauri plugin `ordin` (CommandBridge.md *Public Interface*, *Approach*): one Tauri command per
//! catalogue row, generated from `for_each_command!` so that a capability can allow or deny each one
//! (AuthenticationAndAuthorisation layer 1), plus the two transport commands outside the catalogue,
//! `service_events` and `service_streams`. Each command is one call into the Bridge.
//!
//! Built for macOS and Windows only, the desktop program's targets (HLD 05; FU-010(h)).

use std::collections::BTreeMap;

use ordin_types::BackendId;
use serde_json::value::RawValue;
use tauri::ipc::Channel;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, State, WebviewWindow, WindowEvent};

use crate::error::BridgeError;
use crate::events::{EventBatch, EventsOpened};
use crate::streams::StreamMessage;
use crate::CommandBridge;

/// The plugin's name: commands are invoked as `plugin:ordin|<module>_<verb>`.
pub(crate) const PLUGIN: &str = "ordin";

/// Turns the Bridge's reply into the command's return value. `Err(())` only when there is no reply
/// to send (the window closed while its command ran, step 3) — no one is left to read it.
fn reply(reply: Option<String>) -> Result<Box<RawValue>, ()> {
    reply
        .and_then(|json| RawValue::from_string(json).ok())
        .ok_or(())
}

macro_rules! ipc_plugin {
    ($( ($module:ident, $verb:ident, $method:ident, $input:ty, $result:ty, $scope:ident, $effect:ident); )*) => {
        $(
            /// One catalogue command; the reply carries every error, so the result is always `Ok`
            /// while the window is open.
            #[tauri::command]
            async fn $method<R: Runtime>(
                bridge: State<'_, CommandBridge>,
                window: WebviewWindow<R>,
                envelope: Box<RawValue>,
            ) -> Result<Box<RawValue>, ()> {
                reply(
                    bridge
                        .invoke(
                            window.label(),
                            concat!(stringify!($module), ".", stringify!($verb)),
                            envelope.get(),
                        )
                        .await,
                )
            }
        )*

        /// The plugin carrying every generated command, `service_events` and `service_streams`.
        pub(crate) fn build<R: Runtime>() -> TauriPlugin<R> {
            Builder::new(PLUGIN)
                .invoke_handler(tauri::generate_handler![
                    $( $method, )*
                    service_events,
                    service_streams,
                ])
                .on_window_ready(|window| {
                    // Step 3: a destroyed window's commands are cancelled and its streams close.
                    let label = window.label().to_owned();
                    let app = window.app_handle().clone();
                    window.on_window_event(move |event| {
                        if matches!(event, WindowEvent::Destroyed) {
                            if let Some(bridge) = app.try_state::<CommandBridge>() {
                                bridge.window_destroyed(&label);
                            }
                        }
                    });
                })
                .build()
        }
    };
}

ordin_service_api::for_each_command!(ipc_plugin);

/// Opens the event stream for the calling window. `after` maps a backend id to the last seq the
/// window has drawn; absent backends start live. Replaces any earlier stream of that window.
#[tauri::command]
async fn service_events<R: Runtime>(
    bridge: State<'_, CommandBridge>,
    window: WebviewWindow<R>,
    after: BTreeMap<BackendId, u64>,
    on_events: Channel<EventBatch>,
) -> Result<EventsOpened, BridgeError> {
    bridge
        .open_events(
            window.label(),
            after,
            Box::new(move |batch| on_events.send(batch).map_err(|_| ())),
        )
        .await
}

/// Opens the unjournalled stream for the calling window: dictation updates (stream id = the
/// `session_id` that `dictation.start` returned), terminal bytes and language-server messages
/// (stream id = the Project Service's `StreamId`). Replaces any earlier stream of that window.
/// Nothing on this channel is journalled or replayed (ADR-022, ADR-043).
#[tauri::command]
async fn service_streams<R: Runtime>(
    bridge: State<'_, CommandBridge>,
    window: WebviewWindow<R>,
    on_stream: Channel<StreamMessage>,
) -> Result<(), BridgeError> {
    bridge.open_streams(
        window.label(),
        Box::new(move |message| on_stream.send(message).map_err(|_| ())),
    );
    Ok(())
}
