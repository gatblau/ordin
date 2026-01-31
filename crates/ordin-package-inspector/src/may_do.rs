//! Steps 14 and 15: what an agent may do, and the plain-words difference between two summaries.

use std::collections::BTreeSet;

use crate::model::{MayDo, MayDoChange, MayDoChangeKind, PackageModel};

const READ_ONLY: &str = "read-only";

/// Pure: the may-do summary of a model.
pub fn may_do(model: &PackageModel) -> MayDo {
    let names = |read_only: bool| -> Vec<String> {
        model
            .tools
            .iter()
            .filter(|tool| (tool.permission == READ_ONLY) == read_only)
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    MayDo {
        tools: names(false),
        reads: names(true),
        connections: model
            .connections
            .iter()
            .map(|connection| connection.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        tool_policy_digest: model.tool_policy_sha256.clone(),
        declaration_digest: model.declaration_sha256.clone(),
        authority: model.authority,
    }
}

fn kebab(kind: MayDoChangeKind) -> &'static str {
    match kind {
        MayDoChangeKind::ToolAdded => "may-do-tool-added",
        MayDoChangeKind::ToolRemoved => "may-do-tool-removed",
        MayDoChangeKind::ReadAdded => "may-do-read-added",
        MayDoChangeKind::ReadRemoved => "may-do-read-removed",
        MayDoChangeKind::ConnectionAdded => "may-do-connection-added",
        MayDoChangeKind::ConnectionRemoved => "may-do-connection-removed",
        MayDoChangeKind::PolicyChanged => "may-do-policy-changed",
        MayDoChangeKind::DeclarationChanged => "may-do-declaration-changed",
        MayDoChangeKind::AuthorityChanged => "may-do-authority-changed",
    }
}

fn change(kind: MayDoChangeKind, name: &str) -> MayDoChange {
    MayDoChange {
        kind,
        name: name.to_owned(),
        message_id: kebab(kind),
    }
}

/// Names in `to` that are not in `from`, in byte order.
fn added(from: &[String], to: &[String]) -> Vec<String> {
    let known: BTreeSet<&String> = from.iter().collect();
    to.iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|name| !known.contains(*name))
        .cloned()
        .collect()
}

fn authority_word(authority: ordin_types::domain::agents::Authority) -> &'static str {
    use ordin_types::domain::agents::Authority;
    match authority {
        Authority::DocumentOnly => "document_only",
        Authority::SystemChanging => "system_changing",
        Authority::RoutingOnly => "routing_only",
    }
}

/// Pure: the differences from `installed` to `candidate`, in `MayDoChangeKind` order, then by name.
/// Empty when the two are equal.
pub fn compare_may_do(installed: &MayDo, candidate: &MayDo) -> Vec<MayDoChange> {
    let mut out = Vec::new();
    let lists: [(&[String], &[String], MayDoChangeKind, MayDoChangeKind); 3] = [
        (
            &installed.tools,
            &candidate.tools,
            MayDoChangeKind::ToolAdded,
            MayDoChangeKind::ToolRemoved,
        ),
        (
            &installed.reads,
            &candidate.reads,
            MayDoChangeKind::ReadAdded,
            MayDoChangeKind::ReadRemoved,
        ),
        (
            &installed.connections,
            &candidate.connections,
            MayDoChangeKind::ConnectionAdded,
            MayDoChangeKind::ConnectionRemoved,
        ),
    ];
    for (from, to, add_kind, remove_kind) in lists {
        for name in added(from, to) {
            out.push(change(add_kind, &name));
        }
        for name in added(to, from) {
            out.push(change(remove_kind, &name));
        }
    }
    if installed.tool_policy_digest != candidate.tool_policy_digest {
        out.push(change(MayDoChangeKind::PolicyChanged, ""));
    }
    if installed.declaration_digest != candidate.declaration_digest {
        out.push(change(MayDoChangeKind::DeclarationChanged, ""));
    }
    if installed.authority != candidate.authority {
        out.push(change(
            MayDoChangeKind::AuthorityChanged,
            authority_word(candidate.authority),
        ));
    }
    out
}
