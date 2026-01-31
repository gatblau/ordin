//! The events of Installers.md *Observability* (SA-OBS-01): identifiers, counts, codes and
//! agent-folder-relative paths only; never file content.

use ordin_types::AgentInstanceId;

use crate::types::InstallerKind;

/// The counts of one install or update.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Counts {
    pub created: u64,
    pub replaced: u64,
    pub unchanged: u64,
    pub removed: u64,
    pub backed_up: u64,
}

/// `installers.installed` or `installers.updated`, info.
pub(crate) fn written(
    updated: bool,
    id: AgentInstanceId,
    installer: InstallerKind,
    plan_hash: &str,
    counts: Counts,
    duration_ms: u64,
) {
    let id = id.to_string();
    if updated {
        tracing::info!(
            agent_instance_id = id.as_str(),
            installer = installer.as_str(),
            plan_hash,
            created = counts.created,
            replaced = counts.replaced,
            unchanged = counts.unchanged,
            removed = counts.removed,
            backed_up = counts.backed_up,
            duration_ms,
            "installers.updated"
        );
    } else {
        tracing::info!(
            agent_instance_id = id.as_str(),
            installer = installer.as_str(),
            plan_hash,
            created = counts.created,
            replaced = counts.replaced,
            unchanged = counts.unchanged,
            removed = counts.removed,
            backed_up = counts.backed_up,
            duration_ms,
            "installers.installed"
        );
    }
}

/// `installers.verified` (debug) when nothing differs, else `installers.mismatch` (warn); the
/// fields are the numbers of changed and missing files.
pub(crate) fn verified(id: AgentInstanceId, changed: usize, missing: usize) {
    let id = id.to_string();
    if changed == 0 && missing == 0 {
        tracing::debug!(
            agent_instance_id = id.as_str(),
            changed = 0u64,
            missing = 0u64,
            "installers.verified"
        );
    } else {
        tracing::warn!(
            agent_instance_id = id.as_str(),
            changed = changed as u64,
            missing = missing as u64,
            "installers.mismatch"
        );
    }
}

/// `installers.repaired`, warn, with the counts of the mismatch it repaired.
pub(crate) fn repaired(id: AgentInstanceId, changed: usize, missing: usize, rewritten: usize) {
    tracing::warn!(
        agent_instance_id = id.to_string().as_str(),
        changed = changed as u64,
        missing = missing as u64,
        rewritten = rewritten as u64,
        "installers.repaired"
    );
}

/// `installers.repair_failed`, error.
pub(crate) fn repair_failed(id: AgentInstanceId, error_code: &str) {
    tracing::error!(
        agent_instance_id = id.to_string().as_str(),
        error_code,
        "installers.repair_failed"
    );
}

/// The three path warnings: `installers.import_missing`, `installers.kept_changed_file`,
/// `installers.backup_mismatch`. `agent_instance_id` is absent from a pure render.
pub(crate) fn path_warning(event: PathEvent, id: Option<AgentInstanceId>, path: &str) {
    let id = id.map(|id| id.to_string()).unwrap_or_default();
    match event {
        PathEvent::ImportMissing => {
            tracing::warn!(
                agent_instance_id = id.as_str(),
                path,
                "installers.import_missing"
            )
        }
        PathEvent::KeptChangedFile => tracing::warn!(
            agent_instance_id = id.as_str(),
            path,
            "installers.kept_changed_file"
        ),
        PathEvent::BackupMismatch => {
            tracing::warn!(
                agent_instance_id = id.as_str(),
                path,
                "installers.backup_mismatch"
            )
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PathEvent {
    ImportMissing,
    KeptChangedFile,
    BackupMismatch,
}

/// `installers.task_inputs_unreadable`, warn: a command whose inputs do not read is written unchanged
/// (INST-TF-1). The identifier only, never the file's text.
// spec: ordin-desktop/components/Installers#INST-TF-1
pub(crate) fn task_inputs_unreadable(command_id: &str) {
    tracing::warn!(command_id, "installers.task_inputs_unreadable");
}
