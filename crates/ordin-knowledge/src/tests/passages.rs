//! Amendment January 2026 — page sections are passages (ADR-118): KS-TR-1 and KS-TR-3, each test
//! named as its scenario or error row.

use super::rig::Rig;
use crate::service::KnowledgeService;

/// The step KS-TR-3 adds, run as the Journal's migrator runs it on an installation that indexed its
/// pages before the amendment.
const SECTIONS_AS_PASSAGES: &str = include_str!("../sql/sqlite_v7.sql");

// spec: ordin-desktop/components/KnowledgeService#KS-TR-3
#[test]
fn the_sections_as_passages_step_is_numbered_one_above_the_highest() {
    let m = KnowledgeService::migrations();
    let last = m.sqlite.last().expect("a step");
    assert_eq!((last.0, last.1), (7, "sections as passages"));
    assert_eq!(last.2, SECTIONS_AS_PASSAGES);
    // SQLite slice only, as pages are.
    assert_eq!(m.postgres.len(), 2);
}

// spec: ordin-desktop/components/KnowledgeService#KS-TR-1
#[tokio::test]
async fn happy_path_a_pages_sections_are_passages() {
    let rig = Rig::desktop().await;
    let id = rig
        .write(
            "Rates",
            "Intro.\n\n## Day rate\n\nGBP 600.\n\n## Notice\n\nFour weeks.",
        )
        .await;
    rig.index().await;
    // Nothing this crate sends changes: the sections go to `index_page_sections` as before, and the
    // Memory Service writes them as passages (MemoryService MEM-TR-6).
    let calls = rig.f.memory.calls_for(id);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].sections.len() >= 2, "{:?}", calls[0].sections);
    let row = rig.row(id).await;
    assert_eq!(row.memory_indexed_version, Some(row.file_version));
}

// spec: ordin-desktop/components/KnowledgeService#KS-TR-3
#[tokio::test]
async fn edge_sections_indexed_before_the_amendment_are_written_again() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "Body.").await;
    rig.index().await;
    let version = rig.row(id).await.file_version;
    assert_eq!(rig.f.memory.calls_for(id).len(), 1);

    rig.exec(SECTIONS_AS_PASSAGES).await;
    assert_eq!(rig.row(id).await.memory_indexed_version, None);
    rig.index().await;

    let calls = rig.f.memory.calls_for(id);
    assert_eq!(calls.len(), 2, "indexed again");
    assert_eq!(calls[1].version, calls[0].version, "at the same version");
    let row = rig.row(id).await;
    assert_eq!(row.file_version, version, "no page changes");
    assert_eq!(row.memory_indexed_version, Some(version));
}

// Error row: `index_page_sections` refused while re-indexing (KS-TR-3) — as §F today: the page
// stays not indexed and is tried on the next pass.
// spec: ordin-desktop/components/KnowledgeService#KS-TR-3
#[tokio::test]
async fn error_path_a_refused_reindex_is_tried_on_the_next_pass() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "Body.").await;
    rig.index().await;
    rig.exec(SECTIONS_AS_PASSAGES).await;
    *rig.f.memory.fail_index.lock().unwrap() = 1;
    rig.index().await;
    let row = rig.row(id).await;
    assert_eq!(row.memory_indexed_version, None);
    assert_eq!(row.memory_index_attempts, 1);
    rig.exec("UPDATE knowledge_pages SET memory_next_attempt_at = '2000-01-01T00:00:00.000Z'")
        .await;
    rig.index().await;
    let row = rig.row(id).await;
    assert_eq!(row.memory_indexed_version, Some(row.file_version));
}
