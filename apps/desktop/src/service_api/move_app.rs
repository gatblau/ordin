//! `app.install_place` and `app.move_to_applications` (OrdinDesktop.md DSK-MOVE-1 to DSK-MOVE-3;
//! ServiceApi.md SVC-MOVE-1): ordin moves itself to the person's Applications folder. macOS only;
//! elsewhere ordin counts as installed and nothing is moved.

use std::path::PathBuf;

use ordin_service_api::commands::{InstallPlace, MovedToApplications};
use ordin_service_api::ServiceError;

use super::DesktopServiceApi;
use crate::error::{MOVE_COPY_FAILED, MOVE_NOT_NEEDED, MOVE_START_FAILED};
use crate::move_app::{bundle_of, is_installed, move_bundle, target_of, MoveError, SystemTools};
use crate::shell::QuitTrigger;

/// The running bundle and the person's home folder, on macOS when ordin runs from a bundle.
fn place() -> Option<(PathBuf, PathBuf)> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    let bundle = bundle_of(&std::env::current_exe().ok()?)?;
    Some((bundle, PathBuf::from(home)))
}

impl DesktopServiceApi {
    /// `app.install_place` (DSK-MOVE-1).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-1
    pub(crate) fn app_install_place(&self) -> Result<InstallPlace, ServiceError> {
        Ok(match place() {
            Some((bundle, home)) => InstallPlace {
                installed: is_installed(&bundle, &home),
                target: target_of(&bundle, &home)
                    .map(|t| t.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            },
            None => InstallPlace {
                installed: true,
                target: String::new(),
            },
        })
    }

    /// `app.move_to_applications` (DSK-MOVE-2, DSK-MOVE-3): the copy made off the async runtime, then
    /// the quit with `MovedToApplications` on its own task, so the answer is not held back by it.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-2
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MOVE-3
    pub(crate) async fn app_move_to_applications(
        &self,
    ) -> Result<MovedToApplications, ServiceError> {
        let Some((bundle, home)) = place().filter(|(b, h)| !is_installed(b, h)) else {
            return Err(self.own_refusal(MOVE_NOT_NEEDED, false));
        };
        // `yyyy-mm-dd hh.mm.ss` (UTC), from `YYYY-MM-DDTHH:MM:SS.mmmZ`.
        let wire = ordin_types::time::Timestamp::now().to_wire();
        let stamp = wire
            .get(..19)
            .unwrap_or(&wire)
            .replace('T', " ")
            .replace(':', ".");
        let pid = std::process::id();
        let moved = tokio::task::spawn_blocking(move || {
            move_bundle(&bundle, &home, &stamp, pid, &SystemTools)
        })
        .await;
        match moved {
            Ok(Ok(_)) => {
                tracing::info!(component = "desktop", "moved_to_applications");
                if let Some(shell) = &self.modules.shell {
                    shell.quit(QuitTrigger::MovedToApplications);
                }
                Ok(MovedToApplications { moved: true })
            }
            Ok(Err(MoveError::StartFailed)) => Err(self.own_refusal(MOVE_START_FAILED, false)),
            Ok(Err(MoveError::CopyFailed)) | Err(_) => {
                Err(self.own_refusal(MOVE_COPY_FAILED, true))
            }
        }
    }
}
