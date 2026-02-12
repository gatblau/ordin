//! OH-MENU-2 to OH-MENU-4: **Add to memory** from a menu on a document that has not changed since it
//! was last added answers at once, with **Read it again**; a finished read records what it read.

use ordin_types::time::Timestamp;
use ordin_types::{PlaceId, RunId, WorkspaceId};

use crate::host::{lock, OrdinHost, COMPONENT};
use crate::store::{self, MenuReadRow};

/// OH-MENU-3: the choice that reads the document again.
pub(crate) const READ_AGAIN: &str = "Read it again";
/// OH-MENU-3: the choice that leaves it as it is.
pub(crate) const THATS_FINE: &str = "That's fine";

/// OH-MENU-2: a pending menu read — the document a menu **Add to memory** is about and the hash of
/// what was read for the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuRead {
    pub place_id: PlaceId,
    pub relative_path: String,
    pub content_hash: String,
}

/// OH-MENU-3: what the check found.
pub(crate) enum MenuCheck {
    /// Same content, and the earlier run's facts are still in memory.
    Unchanged(MenuReadRow, MenuRead),
    /// Read before; the content has changed since.
    Changed(MenuReadRow, MenuRead),
    /// Never read from the menu, or the facts it kept have all been forgotten.
    New(MenuRead),
    /// The document or the earlier facts could not be read; the message goes on as if new, with
    /// nothing pending.
    Unknown,
}

/// OH-MENU-3: a date as the person reads it, "11 February 2026".
pub(crate) fn long_date(t: &Timestamp) -> String {
    let local: chrono::DateTime<chrono::Local> = t.0.into();
    local.format("%-d %B %Y").to_string()
}

/// OH-MENU-3: the question for an unchanged document.
pub(crate) fn already_question(file_name: &str, read_at: &Timestamp) -> String {
    format!(
        "“{file_name}” is already in memory from {}. The document has not changed since.",
        long_date(read_at)
    )
}

/// OH-MENU-3: the line before the turn when the document has changed.
pub(crate) fn changed_line(read_at: &Timestamp) -> String {
    format!(
        "Read before on {}; the document has changed since. Keep only facts that are new or changed.",
        long_date(read_at)
    )
}

/// The last part of a place-relative path.
pub(crate) fn file_name(relative_path: &str) -> &str {
    relative_path
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(relative_path)
}

impl OrdinHost {
    /// OH-MENU-3: reads the document's hash and the row of its last menu read.
    // spec: ordin-desktop/components/OrdinHost#OH-MENU-3
    pub(crate) async fn menu_check(
        &self,
        workspace_id: WorkspaceId,
        place_id: PlaceId,
        relative_path: &str,
    ) -> MenuCheck {
        let unknown = |code: &str| {
            tracing::warn!(component = COMPONENT, code, "ordin_host.menu_read_unknown");
            MenuCheck::Unknown
        };
        let file = match self.ports.files.read_file(place_id, relative_path).await {
            Ok(f) => f,
            Err(e) => return unknown(e.code.as_str()),
        };
        let read = MenuRead {
            place_id,
            relative_path: relative_path.to_owned(),
            content_hash: file.hash.0.clone(),
        };
        let row = match self.pool.acquire().await {
            Ok(mut c) => store::menu_read(&mut c, workspace_id, place_id, relative_path).await,
            Err(e) => Err(e),
        };
        let row = match row {
            Ok(Some(row)) => row,
            Ok(None) => return MenuCheck::New(read),
            Err(_) => return unknown("OH_STORE_FAILED"),
        };
        if row.content_hash != read.content_hash {
            return MenuCheck::Changed(row, read);
        }
        match self.ports.memory.remembered_in_run(row.run_id).await {
            Ok(kept) if !kept.items.is_empty() => MenuCheck::Unchanged(row, read),
            Ok(_) => MenuCheck::New(read),
            Err(e) => unknown(e.code.as_str()),
        }
    }

    /// OH-MENU-2: the conversation holds `read` until the person's next message or the first task
    /// a delegation starts.
    pub(crate) fn hold_menu_read(&self, workspace_id: WorkspaceId, read: Option<MenuRead>) {
        if let Some(conv) = lock(&self.state).convs.get_mut(&workspace_id) {
            conv.pending_menu_read = read;
        }
    }

    /// OH-MENU-2: the first task started while a menu read is held takes it.
    pub(crate) fn take_menu_read(&self, workspace_id: WorkspaceId) -> Option<MenuRead> {
        lock(&self.state)
            .convs
            .get_mut(&workspace_id)
            .and_then(|c| c.pending_menu_read.take())
    }

    /// OH-MENU-2: a task that took a menu read has ended `Completed`; the read is recorded when the
    /// run kept at least one fact.
    // spec: ordin-desktop/components/OrdinHost#OH-MENU-2
    pub(crate) async fn record_menu_read(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
        read: &MenuRead,
    ) {
        match self.ports.memory.remembered_in_run(run_id).await {
            Ok(kept) if kept.items.is_empty() => {}
            Ok(_) => {
                let row = MenuReadRow {
                    content_hash: read.content_hash.clone(),
                    run_id,
                    read_at: Timestamp::now(),
                };
                let put = match self.pool.acquire().await {
                    Ok(mut c) => {
                        store::put_menu_read(
                            &mut c,
                            workspace_id,
                            read.place_id,
                            &read.relative_path,
                            &row,
                        )
                        .await
                    }
                    Err(e) => Err(e),
                };
                if put.is_err() {
                    tracing::warn!(
                        component = COMPONENT,
                        code = "OH_STORE_FAILED",
                        "ordin_host.menu_read_not_recorded"
                    );
                }
            }
            Err(e) => tracing::warn!(
                component = COMPONENT,
                code = e.code.as_str(),
                "ordin_host.menu_read_not_recorded"
            ),
        }
    }
}
