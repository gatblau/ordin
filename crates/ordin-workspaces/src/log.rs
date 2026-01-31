//! The component's log events (WorkspaceService.md *Observability*; Logging.md). Each event is one
//! `tracing` call whose message is the event name. No event carries a workspace name, a folder
//! path, a display name, an email address or domain, an origin location or a file name: logs carry
//! identifiers, kinds, codes and counts.

use ordin_types::{ConfluenceLinkId, PlaceId, SourceLinkId, WorkspaceId};

pub(crate) const COMPONENT: &str = "workspaces";

pub(crate) fn workspace_event(event: &'static str, workspace_id: WorkspaceId) {
    tracing::info!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        "{event}"
    );
}

pub(crate) fn source_link_event(event: &'static str, link: SourceLinkId, kind: &'static str) {
    tracing::info!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        kind,
        "{event}"
    );
}

pub(crate) fn source_link_read(link: SourceLinkId, read: u64, not_read: u64, elapsed_ms: u64) {
    tracing::info!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        read,
        not_read,
        elapsed_ms,
        "source_link_read"
    );
}

pub(crate) fn link_item_read_failed(link: SourceLinkId, code: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        code,
        "link_item_read_failed"
    );
}

pub(crate) fn origin_check_done(workspace_id: WorkspaceId, changes: u32, failed: u32) {
    tracing::info!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        changes,
        failed,
        "origin_check_done"
    );
}

pub(crate) fn origin_check_failed(link: SourceLinkId, code: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        code,
        "origin_check_failed"
    );
}

pub(crate) fn grant_set(
    workspace_id: WorkspaceId,
    place_id: PlaceId,
    grantee_kind: &'static str,
    access: &'static str,
) {
    tracing::info!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        place_id = place_id.to_string().as_str(),
        grantee_kind,
        access,
        "grant_set"
    );
}

pub(crate) fn grants_publish_failed(workspace_id: WorkspaceId) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        "grants_publish_failed"
    );
}

pub(crate) fn confluence_link_decided(link: ConfluenceLinkId, state: &'static str) {
    tracing::info!(
        component = COMPONENT,
        confluence_link_id = link.to_string().as_str(),
        state,
        "confluence_link_decided"
    );
}

pub(crate) fn workspace_forget_failed(workspace_id: WorkspaceId) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        "workspace_forget_failed"
    );
}

pub(crate) fn approval_event_invalid(seq: u64) {
    tracing::warn!(component = COMPONENT, seq, "approval_event_invalid");
}

pub(crate) fn journal_entry_failed(kind: &str) {
    tracing::warn!(component = COMPONENT, kind, "journal_entry_failed");
}

pub(crate) fn workspace_event_failed(workspace_id: WorkspaceId) {
    tracing::warn!(
        component = COMPONENT,
        workspace_id = workspace_id.to_string().as_str(),
        "workspace_event_failed"
    );
}

pub(crate) fn repository_remote_unknown(link: SourceLinkId) {
    tracing::warn!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        "repository_remote_unknown"
    );
}

pub(crate) fn working_copy_git_unreadable(link: SourceLinkId) {
    tracing::warn!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        "working_copy_git_unreadable"
    );
}

pub(crate) fn jira_item_local_change(link: SourceLinkId) {
    tracing::info!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        "jira_item_local_change"
    );
}

pub(crate) fn jira_item_brought_up_to_date(link: SourceLinkId) {
    tracing::info!(
        component = COMPONENT,
        source_link_id = link.to_string().as_str(),
        "jira_item_brought_up_to_date"
    );
}

/// A background step that failed and will run again at its next trigger.
pub(crate) fn background_failed(step: &'static str, code: &'static str) {
    tracing::warn!(component = COMPONENT, step, code, "background_failed");
}
