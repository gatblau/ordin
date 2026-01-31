//! The component's log events (FileService.md *Observability*; Logging.md). Each event is one
//! `tracing` call whose message is the event name and whose `component` is `files`. No event
//! carries a path of the person's folders, file content, extracted text, a search query or a
//! snippet: only `place_id`, kinds, counts and error classes. Identifiers are recorded as strings:
//! the line formatter writes a field recorded through `Display` or `Debug` as `<debug>`.

use ordin_types::{PlaceId, RunId, WorkspaceId};

// spec: ordin-desktop/components/FileService#SA-OBS-01
pub(crate) const COMPONENT: &str = "files";

pub(crate) fn place_added(place_id: PlaceId, kind: &str) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        kind,
        "place_added"
    );
}

pub(crate) fn place_removed(place_id: PlaceId) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        "place_removed"
    );
}

pub(crate) fn place_missing(place_id: PlaceId) {
    tracing::warn!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        "place_missing"
    );
}

pub(crate) fn grant_place_missing(workspace_id: WorkspaceId, place_id: PlaceId) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        place_id = place_id.to_string().as_str(),
        "grant_place_missing"
    );
}

pub(crate) fn index_build_started(place_id: PlaceId) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        "index_build_started"
    );
}

pub(crate) fn index_build_finished(
    place_id: PlaceId,
    indexed: u64,
    total: u64,
    skipped: u64,
    elapsed_ms: u64,
) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        indexed,
        total,
        skipped,
        elapsed_ms,
        "index_build_finished"
    );
}

pub(crate) fn index_extract_failed(place_id: PlaceId, kind: &str, error_class: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        kind,
        error_class,
        "index_extract_failed"
    );
}

pub(crate) fn watch_failed(place_id: PlaceId, error_class: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        error_class,
        "watch_failed"
    );
}

pub(crate) fn file_version_written(
    place_id: PlaceId,
    version: u32,
    origin_kind: &'static str,
    run_id: Option<RunId>,
) {
    match run_id {
        Some(run) => tracing::info!(
            component = COMPONENT,
            place_id = place_id.to_string().as_str(),
            version,
            origin_kind,
            run_id = run.to_string().as_str(),
            "file_version_written"
        ),
        None => tracing::info!(
            component = COMPONENT,
            place_id = place_id.to_string().as_str(),
            version,
            origin_kind,
            "file_version_written"
        ),
    }
}

pub(crate) fn grants_published(workspace_id: WorkspaceId, count: usize) {
    tracing::info!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        count,
        "grants_published"
    );
}

pub(crate) fn versions_pruned(removed: u64, bytes_freed: u64) {
    tracing::info!(
        component = COMPONENT,
        removed,
        bytes_freed,
        "versions_pruned"
    );
}

pub(crate) fn work_folder_unreachable(operation: &'static str, error_class: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        operation,
        error_class,
        "work_folder_unreachable"
    );
}

pub(crate) fn file_moved(place_id: PlaceId) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        "file_moved"
    );
}

pub(crate) fn file_move_history_kept(place_id: PlaceId) {
    tracing::warn!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        "file_move_history_kept"
    );
}

pub(crate) fn journal_failed(kind: &str) {
    tracing::warn!(
        component = COMPONENT,
        entry_kind = kind,
        "files.journal_failed"
    );
}
