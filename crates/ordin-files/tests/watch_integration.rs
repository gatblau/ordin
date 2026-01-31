//! Playbook §7.2, Phase 5: a rename seen by the watcher keeps its history. The real watcher
//! (`notify`) runs on a temporary folder; the Journal, Rules Engine and Agent Folder Manager are the
//! real crates over SQLite. The rename is made on disk by the test, as another app would.

mod support;

use std::time::Duration;

use ordin_files::{FileVersionOrigin, PlaceChangeKind, PlaceKind, WriteVersion};
use support::stack::stack;

#[tokio::test(flavor = "multi_thread")]
async fn a_rename_seen_by_the_watcher_keeps_its_history() {
    let s = stack().await;
    let place = s.place("W", PlaceKind::WorkspaceFolder).await;
    let root = std::path::PathBuf::from(&place.path);
    std::fs::create_dir_all(root.join("knowledge")).unwrap();
    std::fs::write(root.join("knowledge/a.md"), b"first text").unwrap();
    // Version 1 is the file as it was, version 2 the run's write.
    s.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "knowledge/a.md".to_owned(),
            bytes: b"second text".to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap();
    s.service.watch(&[place.id]).await.unwrap();
    // Wait for the first index build.
    let mut built = false;
    for _ in 0..200 {
        let c = s.service.coverage(&[place.id]).await.unwrap();
        if c.total >= 1 && c.indexed == c.total {
            built = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(built, "the index was built");
    let mut changes = s.service.subscribe_changes();

    // Another app renames the file.
    std::fs::rename(root.join("knowledge/a.md"), root.join("knowledge/b.md")).unwrap();

    let renamed = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let change = changes.recv().await.expect("an open channel");
            if matches!(change.change, PlaceChangeKind::Renamed { .. }) {
                return change;
            }
        }
    })
    .await
    .expect("the watcher reports the rename");

    assert_eq!(renamed.path, "knowledge/b.md");
    assert_eq!(
        renamed.change,
        PlaceChangeKind::Renamed {
            from: "knowledge/a.md".to_owned()
        }
    );
    assert!(!renamed.by_ordin, "another app made the rename");
    assert_eq!(renamed.version, Some(2));
    let at_new = s
        .service
        .versions(place.id, "knowledge/b.md")
        .await
        .unwrap();
    assert_eq!(
        at_new.iter().map(|v| v.version).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(s
        .service
        .versions(place.id, "knowledge/a.md")
        .await
        .unwrap()
        .is_empty());
    // The history is usable: version 1 still gives back the first text.
    let first = s.service.read_version(at_new[0].id).await.unwrap();
    assert_eq!(first.bytes, b"first text");
    // The index follows too: search finds the file under its new name only.
    let found = s.service.search(&[place.id], "second", 5).await.unwrap();
    assert_eq!(found.hits.len(), 1);
    assert_eq!(found.hits[0].path, "knowledge/b.md");
    s.service.shutdown().await;
}
