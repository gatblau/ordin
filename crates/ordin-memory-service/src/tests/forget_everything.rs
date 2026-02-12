//! Amendment 2026-02-12 — forget everything, for one workspace or every workspace (MEM-FE-1 to
//! MEM-FE-4), each test named as its scenario or Error Table row.

use super::support::*;
use crate::{ForgetTarget, MemoryPlace};

async fn listed(rig: &Rig, w: ordin_types::WorkspaceId) -> Vec<crate::FactView> {
    rig.svc.list_facts(w, None).await.expect("list").items
}

async fn refs_of(rig: &Rig, w: ordin_types::WorkspaceId) -> i64 {
    count_of(
        rig,
        "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE workspace_id = $1",
        w,
    )
    .await
}

/// A count bound to a workspace id, in the form the store keeps it.
async fn count_of(rig: &Rig, sql: &str, w: ordin_types::WorkspaceId) -> i64 {
    crate::store::count(&rig.svc.inner.db, crate::db::q(sql).uuid(w.0))
        .await
        .expect("count") as i64
}

// spec: ordin-desktop/components/MemoryService#MEM-FE-2
// spec: ordin-desktop/components/MemoryService#MEM-FE-3
// spec: ordin-desktop/components/MemoryService#MEM-FE-4
// Scenario: One workspace is emptied and another is untouched
#[tokio::test]
async fn one_workspace_is_emptied_and_another_is_untouched() {
    let rig = Rig::desktop().await;
    let a = rig.fact(rig.w1, "Kestrel's day rate is GBP 600.").await;
    rig.fact(rig.w1, "Kestrel's notice period is four weeks.")
        .await;
    let wide = rig.svc.remember_everywhere(a).await.expect("widen").id;
    rig.kept(wide).await;
    let check = rig.fact(rig.w1, "Kestrel is based in Leeds.").await;
    rig.fact(rig.w2, "Osprey invoices monthly.").await;
    rig.fact(rig.w2, "Osprey's contact is Ana Silva.").await;
    assert!(
        listed(&rig, rig.w2).await.iter().any(|f| f.id == wide),
        "the fact remembered everywhere is found from B before"
    );

    // Last, so no later write waits for this row to settle.
    rig.svc
        .inner
        .db
        .exec(
            // A write not stored yet: the reconcile pass leaves it pending (§6).
            crate::db::q(
                "UPDATE memory_fact_refs SET state = 'pending', \
                 memory_fact_id = 'not-stored-yet' WHERE id = $1",
            )
            .uuid(check.0),
        )
        .await
        .expect("to check");
    let kept_in_a = count_of(
        &rig,
        "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE workspace_id = $1 AND state = 'kept'",
        rig.w1,
    )
    .await;
    assert!(kept_in_a > 0, "A has kept facts");
    let count = rig
        .svc
        .count_everything(ForgetTarget::Workspace(rig.w1))
        .await
        .expect("count");
    assert_eq!(count.workspaces, 1);
    assert_eq!(count.facts, kept_in_a as u64);
    assert_eq!(count.everywhere, 1);
    assert_eq!(count.to_check, 1);

    let done = rig
        .svc
        .forget_everything(ForgetTarget::Workspace(rig.w1))
        .await
        .expect("forget");
    assert_eq!(done.workspaces, 1);
    assert_eq!(done.facts_removed, count.facts);
    assert_eq!(done.everywhere_removed, 1);
    assert_eq!(refs_of(&rig, rig.w1).await, 0, "A has no facts");
    assert!(listed(&rig, rig.w1).await.is_empty());
    assert!(rig
        .svc
        .held_writes(MemoryPlace::Workspace(rig.w1))
        .await
        .expect("held")
        .is_empty());
    assert_eq!(
        count_of(
            &rig,
            "SELECT COUNT(*) AS n FROM memory_recalls WHERE workspace_id = $1",
            rig.w1
        )
        .await,
        0,
        "no recalls"
    );
    assert_eq!(refs_of(&rig, rig.w2).await, 2, "B still has 2 kept facts");
    assert!(
        !listed(&rig, rig.w2).await.iter().any(|f| f.id == wide),
        "the fact remembered everywhere is no longer found from B"
    );
    let journal = rig.entries("memory_workspace_forgotten").await;
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].workspace_id, Some(rig.w1));
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-FE-3
// Scenario: The workspace's link words stay
#[tokio::test]
async fn the_workspaces_link_words_stay() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Kestrel supplies steel.").await;
    rig.svc
        .vocabulary(MemoryPlace::Workspace(rig.w1))
        .await
        .expect("seed");
    let words = format!(
        "SELECT COUNT(*) AS n FROM memory_terms WHERE place_key = '{}'",
        crate::store::scope_key(MemoryPlace::Workspace(rig.w1))
    );
    let before = rig.count(&words).await;
    assert!(before > 0, "the workspace has link words");
    rig.svc
        .forget_everything(ForgetTarget::Workspace(rig.w1))
        .await
        .expect("forget");
    assert_eq!(
        rig.count(&words).await,
        before,
        "the link words are still listed"
    );
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-FE-1
// Scenario: Every workspace
#[tokio::test]
async fn every_workspace() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Kestrel's day rate is GBP 600.").await;
    rig.fact(rig.w2, "Osprey invoices monthly.").await;
    let done = rig
        .svc
        .forget_everything(ForgetTarget::EveryWorkspace)
        .await
        .expect("forget");
    assert_eq!(done.workspaces, 2);
    let mut ids = vec![rig.w1, rig.w2];
    ids.sort_by_key(|w| w.0);
    assert_eq!(done.workspace_ids, ids, "in workspace-id order");
    assert_eq!(refs_of(&rig, rig.w1).await, 0);
    assert_eq!(refs_of(&rig, rig.w2).await, 0);
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-FE-1
// Error row: not the desktop program — MEM_NOT_SUPPORTED, nothing forgotten.
#[tokio::test]
async fn error_not_the_desktop() {
    for program in [
        crate::config::ProgramKind::PersonalServer,
        crate::config::ProgramKind::AgentSingle,
    ] {
        let rig = Rig::with(Options {
            program,
            ..Options::default()
        })
        .await;
        let w = rig.w1;
        let (code, _) = code_of(rig.svc.forget_everything(ForgetTarget::Workspace(w)).await);
        assert_eq!(code, "MEM_NOT_SUPPORTED");
        let (code, _) = code_of(rig.svc.count_everything(ForgetTarget::EveryWorkspace).await);
        assert_eq!(code, "MEM_NOT_SUPPORTED");
        rig.stop().await;
    }
}

// spec: ordin-desktop/components/MemoryService#MEM-FE-3
// Edge: forgetting a workspace with nothing in it answers zero and journals it.
#[tokio::test]
async fn edge_nothing_to_forget() {
    let rig = Rig::desktop().await;
    let count = rig
        .svc
        .count_everything(ForgetTarget::Workspace(rig.w1))
        .await
        .expect("count");
    assert_eq!((count.facts, count.everywhere, count.to_check), (0, 0, 0));
    let done = rig
        .svc
        .forget_everything(ForgetTarget::Workspace(rig.w1))
        .await
        .expect("forget");
    assert_eq!(done.facts_removed, 0);
    rig.stop().await;
}
