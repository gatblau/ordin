//! The component's log events (ProjectService.md *Observability*; Logging.md). Each event is one
//! `tracing` call whose message is the event name. No event carries a token, file content, a diff,
//! a hunk, a commit message, check output, terminal bytes, `gh` or `glab` arguments or an LSP
//! payload. Identifiers are recorded as strings.

use ordin_types::{ChangeSetEntryId, CodingTaskId, PlaceId, ProjectId};

pub(crate) const COMPONENT: &str = "projects";

pub(crate) fn project_opened(project_id: ProjectId, is_git: bool) {
    tracing::info!(
        component = COMPONENT,
        project_id = project_id.to_string().as_str(),
        is_git,
        "project_opened"
    );
}

pub(crate) fn coding_task_started(task_id: CodingTaskId) {
    tracing::info!(
        component = COMPONENT,
        task_id = task_id.to_string().as_str(),
        "coding_task_started"
    );
}

pub(crate) fn coding_task_removed(task_id: CodingTaskId) {
    tracing::info!(
        component = COMPONENT,
        task_id = task_id.to_string().as_str(),
        "coding_task_removed"
    );
}

pub(crate) fn change_set_ready(
    task_id: CodingTaskId,
    entries: usize,
    new_problems: u32,
    test_outcome: Option<&'static str>,
) {
    tracing::info!(
        component = COMPONENT,
        task_id = task_id.to_string().as_str(),
        entries,
        new_problems,
        test_outcome = test_outcome.unwrap_or("none"),
        "change_set_ready"
    );
}

pub(crate) fn entry_decided(
    entry_id: ChangeSetEntryId,
    decision: &'static str,
    outcome: &'static str,
) {
    tracing::info!(
        component = COMPONENT,
        entry_id = entry_id.to_string().as_str(),
        decision,
        outcome,
        "entry_decided"
    );
}

pub(crate) fn check_run(
    project_id: ProjectId,
    task_id: Option<CodingTaskId>,
    exit_code: Option<i32>,
    elapsed_ms: u64,
) {
    tracing::info!(
        component = COMPONENT,
        project_id = project_id.to_string().as_str(),
        task_id = task_id.map(|t| t.to_string()).unwrap_or_default().as_str(),
        exit_code = exit_code.unwrap_or(-1),
        elapsed_ms,
        "check_run"
    );
}

pub(crate) fn git_fetch(place_id: PlaceId, branch: &str, result: &'static str, elapsed_ms: u64) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        branch,
        result,
        elapsed_ms,
        "git_fetch"
    );
}

pub(crate) fn original_changed(place_id: PlaceId, commit: &str) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        commit,
        "original_changed"
    );
}

pub(crate) fn git_pushed(place_id: PlaceId, branch: &str) {
    tracing::info!(
        component = COMPONENT,
        place_id = place_id.to_string().as_str(),
        branch,
        "git_pushed"
    );
}

pub(crate) fn git_host_call(server: &str, exit_code: i32) {
    tracing::info!(component = COMPONENT, server, exit_code, "git_host_call");
}

/// Internal Logic 14.5: no repository name, path or content.
pub(crate) fn git_host_api(
    operation: &'static str,
    host_kind: &'static str,
    status: u16,
    elapsed_ms: u64,
) {
    tracing::info!(
        component = COMPONENT,
        operation,
        host_kind,
        status,
        elapsed_ms,
        "git_host_api"
    );
}

/// ADR-110 14.6: the answer's kind, never the repository name.
pub(crate) fn branch_review_answer(host_kind: &'static str, answer: &'static str) {
    tracing::info!(
        component = COMPONENT,
        operation = "branch_review_status",
        host_kind,
        answer,
        "git_host_api"
    );
}

pub(crate) fn language_server(
    event: LanguageServerEvent,
    language: &str,
    server: &str,
    instance: u64,
) {
    match event {
        LanguageServerEvent::Started => tracing::info!(
            component = COMPONENT,
            language,
            server,
            instance,
            "language_server_started"
        ),
        LanguageServerEvent::Crashed => tracing::warn!(
            component = COMPONENT,
            language,
            server,
            instance,
            "language_server_crashed"
        ),
        LanguageServerEvent::Stopped => tracing::info!(
            component = COMPONENT,
            language,
            server,
            instance,
            "language_server_stopped"
        ),
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LanguageServerEvent {
    Started,
    Crashed,
    Stopped,
}

pub(crate) fn ide_opened(ide_id: &str) {
    tracing::info!(component = COMPONENT, ide_id, "ide_opened");
}

/// C-32: an IDE whose start failed leaves the list.
pub(crate) fn ide_dropped(ide_id: &str) {
    tracing::warn!(component = COMPONENT, ide_id, "ide_dropped");
}

/// Internal Logic 3.1: a remote on a host ordin does not serve. The host is not logged.
pub(crate) fn unsupported_host() {
    tracing::warn!(component = COMPONENT, "unsupported_host");
}

pub(crate) fn git_failed(class: &'static str) {
    tracing::warn!(component = COMPONENT, class, "git_failed");
}

/// A journal failure; the operation it records stands.
pub(crate) fn journal_failed(kind: &'static str, code: &'static str) {
    tracing::error!(component = COMPONENT, kind, code, "journal_failed");
}

/// A background step that failed and will run again at its next trigger.
pub(crate) fn background_failed(step: &'static str, code: &'static str) {
    tracing::warn!(component = COMPONENT, step, code, "background_failed");
}
