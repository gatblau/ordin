//! Amendment 2026-02-12 — pages are indexed again after memory is forgotten (KS-FE-1), each test
//! named as its scenario or error row.

use super::rig::Rig;

// spec: ordin-desktop/components/KnowledgeService#KS-FE-1
#[tokio::test]
async fn every_page_of_the_workspace_is_indexed_again() {
    let rig = Rig::desktop().await;
    let rates = rig.write("Rates", "Body.").await;
    let budget = rig.write("Budget", "Body.").await;
    let old = rig.write("Old", "Body.").await;
    rig.index().await;
    rig.svc.remove_page(old).await.expect("removed");

    let marked = rig.svc.index_again(rig.w).await.expect("index again");
    assert_eq!(marked, 2);
    assert_eq!(rig.row(rates).await.memory_indexed_version, None);
    rig.index().await;
    for id in [rates, budget] {
        let row = rig.row(id).await;
        assert_eq!(row.memory_indexed_version, Some(row.file_version));
        assert_eq!(rig.f.memory.calls_for(id).len(), 2, "indexed again");
    }
    assert_eq!(
        rig.f.memory.calls_for(old).len(),
        1,
        "the removed page is not indexed"
    );
    assert!(rig
        .kinds()
        .await
        .iter()
        .any(|k| k == "knowledge_pages_to_index_again"));
}

// spec: ordin-desktop/components/KnowledgeService#KS-FE-1
#[tokio::test]
async fn edge_another_workspace_is_untouched() {
    let rig = Rig::desktop().await;
    let id = rig.write("Plan", "Body.").await;
    rig.index().await;
    let (other, _) = rig.workspace("Other");
    rig.svc.index_again(other).await.expect("index again");
    let row = rig.row(id).await;
    assert_eq!(row.memory_indexed_version, Some(row.file_version));
}
