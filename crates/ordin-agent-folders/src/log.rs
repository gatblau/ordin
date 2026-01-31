//! The component's log events (AgentFolderManager.md *Observability*; Logging.md). Each event is
//! one `tracing` call whose message is the event name. No event carries a path of the person's
//! files, only place and instance identifiers. Identifiers are recorded as strings: the line
//! formatter writes a field recorded through `Display` or `Debug` as `<debug>`.

use ordin_types::{AgentInstanceId, FolderGrantId, PlaceId, RunId, WorkspaceId};

use crate::MODULE;

pub(crate) fn created(id: AgentInstanceId, duration_ms: u64) {
    tracing::info!(
        component = MODULE,
        agent_instance_id = id.to_string().as_str(),
        duration_ms,
        "agent_folders.created"
    );
}

pub(crate) fn removed(id: AgentInstanceId, duration_ms: u64) {
    tracing::info!(
        component = MODULE,
        agent_instance_id = id.to_string().as_str(),
        duration_ms,
        "agent_folders.removed"
    );
}

pub(crate) fn lock_busy(id: AgentInstanceId, requested_mode: &'static str, held: &'static str) {
    tracing::warn!(
        component = MODULE,
        agent_instance_id = id.to_string().as_str(),
        requested_mode,
        held,
        "agent_folders.lock_busy"
    );
}

pub(crate) fn grants_applied(
    run_id: RunId,
    id: AgentInstanceId,
    workspace_id: WorkspaceId,
    grants: usize,
) {
    tracing::info!(
        component = MODULE,
        run_id = run_id.to_string().as_str(),
        agent_instance_id = id.to_string().as_str(),
        workspace_id = workspace_id.to_string().as_str(),
        grants,
        "agent_folders.grants_applied"
    );
}

pub(crate) fn unknown_directory(directory_name: &str) {
    tracing::warn!(
        component = MODULE,
        directory_name,
        "agent_folders.unknown_directory"
    );
}

/// Step 6a: a grant left out of the stored set.
pub(crate) fn grant_skipped_stored(folder_grant_id: FolderGrantId, place_id: PlaceId) {
    tracing::warn!(
        component = MODULE,
        folder_grant_id = folder_grant_id.to_string().as_str(),
        place_id = place_id.to_string().as_str(),
        "agent_folders.grant_skipped"
    );
}

/// Step 6: a granted place left out of a session.
pub(crate) fn grant_skipped_session(place_id: PlaceId) {
    tracing::warn!(
        component = MODULE,
        place_id = place_id.to_string().as_str(),
        "agent_folders.grant_skipped"
    );
}

pub(crate) fn grant_unreachable(place_id: PlaceId) {
    tracing::warn!(
        component = MODULE,
        place_id = place_id.to_string().as_str(),
        "agent_folders.grant_unreachable"
    );
}

pub(crate) fn workspace_grants_set(
    workspace_id: WorkspaceId,
    grants: usize,
    skipped: usize,
    sessions_updated: usize,
) {
    tracing::info!(
        component = MODULE,
        workspace_id = workspace_id.to_string().as_str(),
        grants,
        skipped,
        sessions_updated,
        "agent_folders.workspace_grants_set"
    );
}

/// Step 10: a datastore failure, with the statement name only.
pub(crate) fn datastore_failed(statement: &'static str, retryable: bool) {
    tracing::error!(
        component = MODULE,
        statement,
        retryable,
        "agent_folders.datastore_failed"
    );
}

/// A journal failure on one of the four entry kinds; the operation it records stands.
pub(crate) fn journal_failed(kind: &'static str, code: &'static str) {
    tracing::error!(
        component = MODULE,
        kind,
        code,
        "agent_folders.journal_failed"
    );
}
