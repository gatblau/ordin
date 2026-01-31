//! Behaviour behind the Gherkin scenarios: listing in both modes, previews, the index and the
//! watcher's batches, versions and pruning, import, fetched writes, copies and the small rules.

use std::time::Duration;

use ordin_journal::PruneRequest;
use ordin_types::time::Timestamp;
use ordin_types::{ConfluenceLinkId, KnowledgePageId, PublishedCopyId, RunId};

use super::support::*;
use crate::types::{AddPlace, FolderSort, PlaceChangeKind, Preview, WriteVersion};
use crate::{
    AgentWriteTarget, CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileVersionOrigin,
    Place, PlaceKind, PublishedCopy, PublishedCopyState, PublishedVia,
};

async fn write(fx: &Fixture, place: &Place, path: &str, bytes: &[u8]) -> crate::FileVersion {
    fx.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: path.to_owned(),
            bytes: bytes.to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap()
}

fn names(page: &crate::Page<crate::FileEntry>) -> Vec<String> {
    page.items.iter().map(|e| e.name.clone()).collect()
}

#[tokio::test]
async fn listing_gives_the_same_pages_from_the_disk_and_from_the_index() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "b.txt", b"bb");
    fx.put(&place, "a.md", b"a");
    fx.put(&place, "c.csv", b"ccc");
    fx.put(&place, "sub/inner.txt", b"x");
    write(&fx, &place, "b.txt", b"bb, changed").await;
    fx.service
        .set_attribution(place.id, "c.csv", "Asha Patel".into())
        .await
        .unwrap();
    let sorts = [
        FolderSort::NameAsc,
        FolderSort::NameDesc,
        FolderSort::KindAsc,
        FolderSort::ChangedDesc,
        FolderSort::ChangedAsc,
        FolderSort::ChangedByAsc,
    ];
    let mut from_disk = Vec::new();
    for sort in sorts {
        from_disk.push(names(
            &fx.service
                .list_folder(place.id, "", sort, None)
                .await
                .unwrap(),
        ));
    }
    assert_eq!(from_disk[0], ["a.md", "b.txt", "c.csv", "sub"]);
    assert_eq!(from_disk[1], ["sub", "c.csv", "b.txt", "a.md"]);
    fx.service.rescan_place(place.id).await.unwrap();
    for (n, sort) in sorts.iter().enumerate() {
        let page = fx
            .service
            .list_folder(place.id, "", *sort, None)
            .await
            .unwrap();
        if matches!(
            sort,
            FolderSort::NameAsc | FolderSort::NameDesc | FolderSort::KindAsc
        ) {
            assert_eq!(names(&page), from_disk[n], "{sort:?}");
        } else {
            assert_eq!(page.items.len(), 4, "{sort:?}");
        }
    }
    // The indexed page carries the entry's fields.
    let page = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, None)
        .await
        .unwrap();
    let b = page.items.iter().find(|e| e.name == "b.txt").unwrap();
    assert!(b.has_versions && !b.is_folder);
    assert_eq!((b.kind.as_str(), b.size_bytes), ("txt", 11));
    assert_eq!(b.changed_by.as_deref(), Some("you"));
    let c = page.items.iter().find(|e| e.name == "c.csv").unwrap();
    assert_eq!(c.changed_by.as_deref(), Some("Asha Patel"));
    let sub = page.items.iter().find(|e| e.name == "sub").unwrap();
    assert!(sub.is_folder && sub.path == "sub/" && sub.kind.is_empty());
    let by_author = fx
        .service
        .list_folder(place.id, "", FolderSort::ChangedByAsc, None)
        .await
        .unwrap();
    assert_eq!(
        by_author.items[0].changed_by, None,
        "unknown authors sort first"
    );
    let inner = fx
        .service
        .list_folder(place.id, "sub/", FolderSort::NameAsc, None)
        .await
        .unwrap();
    assert_eq!(names(&inner), ["inner.txt"]);
    assert_eq!(inner.items[0].path, "sub/inner.txt");
    assert_eq!(
        fx.service
            .list_folder(place.id, "none", FolderSort::NameAsc, None)
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
}

#[tokio::test]
async fn paging_by_every_sort_walks_the_whole_folder_once() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    for n in 0..230 {
        let ext = ["md", "txt", "csv"][n % 3];
        fx.put(&place, &format!("f{n:03}.{ext}"), format!("{n}").as_bytes());
    }
    for indexed in [false, true] {
        if indexed {
            fx.service.rescan_place(place.id).await.unwrap();
        }
        for sort in [
            FolderSort::NameAsc,
            FolderSort::NameDesc,
            FolderSort::KindAsc,
            FolderSort::ChangedDesc,
            FolderSort::ChangedAsc,
            FolderSort::ChangedByAsc,
        ] {
            let mut seen = std::collections::HashSet::new();
            let mut cursor = None;
            let mut pages = 0;
            loop {
                let page = fx
                    .service
                    .list_folder(place.id, "", sort, cursor)
                    .await
                    .unwrap();
                pages += 1;
                for item in &page.items {
                    assert!(
                        seen.insert(item.path.clone()),
                        "{sort:?} repeats {}",
                        item.path
                    );
                }
                match page.next_cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            assert_eq!((seen.len(), pages), (230, 3), "{sort:?} indexed={indexed}");
        }
    }
}

#[tokio::test]
async fn reading_media_types_and_display_paths() {
    let fx = fixture().await;
    let place = fx.place("Supplier quotes", PlaceKind::PersonsFolder).await;
    fx.put(&place, "Q3/q.pdf", b"%PDF-1.4");
    let read = fx.service.read_file(place.id, "/Q3//q.pdf").await.unwrap();
    assert_eq!(read.path, "Q3/q.pdf");
    assert_eq!(read.media_type, "application/pdf");
    assert_eq!(read.hash.0, crate::versions::hash_of(b"%PDF-1.4"));
    fx.put(&place, "x.unknown", b"?");
    assert_eq!(
        fx.service
            .read_file(place.id, "x.unknown")
            .await
            .unwrap()
            .media_type,
        "application/octet-stream"
    );
    assert_eq!(
        fx.service.display_path(place.id, "Q3/q.pdf").await.unwrap(),
        "Supplier quotes › Q3 › q.pdf"
    );
    assert_eq!(
        fx.service.display_path(place.id, "").await.unwrap(),
        "Supplier quotes"
    );
    assert_eq!(
        fx.service
            .read_file(place.id, "Q3")
            .await
            .unwrap_err()
            .code(),
        "FVER_PATH_INVALID",
        "a folder is not a file"
    );
}

#[tokio::test]
async fn previews_by_kind() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "pic.png", b"\x89PNG");
    fx.put(&place, "logo.svg", b"<svg onload=alert(1)></svg>");
    fx.put(&place, "doc.pdf", b"%PDF-1.4");
    fx.put(&place, "note.md", b"# Title");
    fx.put(&place, "data.json", b"{}");
    fx.put(&place, "blob.bin", b"\x00\x01");
    let big_text = "x".repeat(crate::PREVIEW_TEXT_BYTES + 100);
    fx.put(&place, "big.txt", big_text.as_bytes());
    match fx.service.preview(place.id, "pic.png").await.unwrap() {
        Preview::Image {
            media_type,
            bytes_b64,
        } => {
            assert_eq!(media_type, "image/png");
            assert_eq!(bytes_b64, "iVBORw==");
        }
        other => panic!("{other:?}"),
    }
    // An SVG is image bytes, never markup.
    assert!(matches!(
        fx.service.preview(place.id, "logo.svg").await.unwrap(),
        Preview::Image { media_type, .. } if media_type == "image/svg+xml"
    ));
    assert!(matches!(
        fx.service.preview(place.id, "doc.pdf").await.unwrap(),
        Preview::Pdf { .. }
    ));
    assert_eq!(
        fx.service.preview(place.id, "note.md").await.unwrap(),
        Preview::Text {
            text: "# Title".into(),
            markdown: true,
            truncated: false
        }
    );
    assert!(matches!(
        fx.service.preview(place.id, "data.json").await.unwrap(),
        Preview::Text {
            markdown: false,
            ..
        }
    ));
    match fx.service.preview(place.id, "big.txt").await.unwrap() {
        Preview::Text {
            text, truncated, ..
        } => {
            assert!(truncated);
            assert_eq!(text.len(), crate::PREVIEW_TEXT_BYTES);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        fx.service.preview(place.id, "blob.bin").await.unwrap(),
        Preview::None
    );
    // An Office file gets a Quick Look thumbnail on macOS when it can; otherwise a Word, Excel or
    // PowerPoint file shows its text, one paragraph a line (FS-PV-1), and one that is not readable nothing.
    fx.put(&place, "memo.docx", b"not really a document");
    let office = fx.service.preview(place.id, "memo.docx").await.unwrap();
    assert!(
        matches!(office, Preview::None | Preview::Thumbnail { .. }),
        "{office:?}"
    );
    fx.put(
        &place,
        "terms.docx",
        &word_file(&["Supplier terms", "Payment within 30 days."]),
    );
    match fx.service.preview(place.id, "terms.docx").await.unwrap() {
        Preview::Text {
            text,
            markdown,
            truncated,
        } => {
            assert_eq!(text, "Supplier terms\nPayment within 30 days.");
            assert!(!markdown && !truncated);
        }
        Preview::Thumbnail { .. } if cfg!(target_os = "macos") => {}
        other => panic!("{other:?}"),
    }
    fx.put(&place, "old.doc", b"\xd0\xcf\x11\xe0");
    let old = fx.service.preview(place.id, "old.doc").await.unwrap();
    assert!(
        matches!(old, Preview::None | Preview::Thumbnail { .. }),
        "{old:?}"
    );
}

/// A Word file whose body holds one paragraph per line of `paragraphs`.
fn word_file(paragraphs: &[&str]) -> Vec<u8> {
    use std::io::Write as _;
    let body: String = paragraphs
        .iter()
        .map(|p| format!("<w:p><w:r><w:t>{p}</w:t></w:r></w:p>"))
        .collect();
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        write!(
            writer,
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        )
        .unwrap();
        writer.finish().unwrap();
    }
    out
}

#[tokio::test]
async fn the_rescan_finds_new_changed_and_removed_files_and_folders() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "keep.txt", b"keep");
    fx.put(&place, "change.txt", b"before");
    fx.put(&place, "gone.txt", b"gone");
    fx.put(&place, "dir/deep/inner.md", b"inner words");
    fx.service.rescan_place(place.id).await.unwrap();
    assert_eq!(
        fx.service.place_row(place.id).await.unwrap().index_state,
        "ready"
    );
    let coverage = fx.service.coverage(&[place.id]).await.unwrap();
    assert_eq!((coverage.indexed, coverage.total), (4, 4));
    let mut changes = fx.service.subscribe_changes();
    std::fs::write(fx.root(&place).join("change.txt"), b"after, and longer").unwrap();
    std::fs::remove_file(fx.root(&place).join("gone.txt")).unwrap();
    fx.put(&place, "fresh.txt", b"fresh");
    fx.service.rescan_place(place.id).await.unwrap();
    let mut seen = Vec::new();
    for _ in 0..3 {
        let c = next_change(&mut changes).await;
        assert!(!c.by_ordin);
        seen.push((c.path, c.change));
    }
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        vec![
            ("change.txt".to_owned(), PlaceChangeKind::Modified),
            ("fresh.txt".to_owned(), PlaceChangeKind::Created),
            ("gone.txt".to_owned(), PlaceChangeKind::Removed),
        ]
    );
    assert_eq!(fx.service.coverage(&[place.id]).await.unwrap().total, 4);
    // Text of nested files is searchable and gone files are not.
    let found = fx
        .service
        .search(&[place.id], "inner words", 10)
        .await
        .unwrap();
    assert_eq!(found.hits.len(), 1);
    assert!(fx
        .service
        .search(&[place.id], "gone", 10)
        .await
        .unwrap()
        .hits
        .is_empty());
}

#[tokio::test]
async fn a_place_whose_folder_is_missing_is_left_alone_by_the_rescan() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    std::fs::remove_dir_all(fx.root(&place)).unwrap();
    fx.service.rescan_place(place.id).await.unwrap();
    assert_eq!(
        fx.service.place_row(place.id).await.unwrap().index_state,
        "none"
    );
}

#[tokio::test]
async fn the_watcher_batch_handles_created_modified_removed_and_whole_folders() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "old.txt", b"old");
    fx.put(&place, "tree/a.txt", b"a");
    fx.service.rescan_place(place.id).await.unwrap();
    let mut changes = fx.service.subscribe_changes();

    // New file, changed file, removed file, a folder that appears whole, a folder that goes.
    fx.put(&place, "new.txt", b"new");
    std::fs::write(fx.root(&place).join("old.txt"), b"old, then longer").unwrap();
    fx.put(&place, "moved-in/x.md", b"x words");
    fx.put(&place, "moved-in/nested/y.md", b"y");
    std::fs::remove_dir_all(fx.root(&place).join("tree")).unwrap();
    fx.service
        .process_batch(
            place.id,
            [
                "new.txt",
                "old.txt",
                "moved-in",
                "tree",
                "tree/a.txt",
                "absent.txt",
            ]
            .map(String::from)
            .to_vec(),
        )
        .await
        .unwrap();
    let mut seen = Vec::new();
    while let Ok(Ok(c)) = tokio::time::timeout(Duration::from_millis(300), changes.recv()).await {
        seen.push((c.path, c.change));
    }
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        vec![
            ("moved-in/nested/y.md".to_owned(), PlaceChangeKind::Created),
            ("moved-in/x.md".to_owned(), PlaceChangeKind::Created),
            ("new.txt".to_owned(), PlaceChangeKind::Created),
            ("old.txt".to_owned(), PlaceChangeKind::Modified),
            ("tree/a.txt".to_owned(), PlaceChangeKind::Removed),
        ]
    );
    assert_eq!(
        fx.service
            .search(&[place.id], "x words", 5)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    // An unchanged file, and a path that is quiet, produce nothing.
    fx.service.mark_quiet(place.id, "new.txt");
    std::fs::write(
        fx.root(&place).join("new.txt"),
        b"changed under a quiet mark",
    )
    .unwrap();
    fx.service
        .process_batch(place.id, vec!["new.txt".into(), "old.txt".into()])
        .await
        .unwrap();
    assert!(no_change(&mut changes, Duration::from_millis(200)).await);
}

#[tokio::test]
async fn a_person_s_change_to_a_file_with_history_becomes_a_version_unless_ordin_wrote_it() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.md", b"one");
    let mut changes = fx.service.subscribe_changes();
    write(&fx, &place, "a.md", b"two").await;
    let _ = next_change(&mut changes).await; // the write's own announcement
                                             // The write made here is not announced a second time.
    fx.service
        .process_batch(place.id, vec!["a.md".into()])
        .await
        .unwrap();
    assert!(no_change(&mut changes, Duration::from_millis(200)).await);
    std::fs::write(fx.root(&place).join("a.md"), b"three, by the person").unwrap();
    fx.service
        .process_batch(place.id, vec!["a.md".into()])
        .await
        .unwrap();
    let c = next_change(&mut changes).await;
    assert_eq!(
        (c.version, c.origin),
        (Some(3), Some(FileVersionOrigin::Person))
    );
    // A file with no history gets no version.
    fx.put(&place, "plain.md", b"plain");
    fx.service.rescan_place(place.id).await.unwrap();
    std::fs::write(fx.root(&place).join("plain.md"), b"plain, edited").unwrap();
    fx.service
        .process_batch(place.id, vec!["plain.md".into()])
        .await
        .unwrap();
    let c = next_change(&mut changes).await;
    assert_eq!(
        (c.path.as_str(), c.version, c.origin),
        ("plain.md", None, None)
    );
}

#[tokio::test]
async fn the_real_watcher_reports_a_new_file_after_the_debounce() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "seed.txt", b"seed");
    let mut changes = fx.service.subscribe_changes();
    fx.service.watch(&[place.id]).await.unwrap();
    wait_ready(&fx, &place).await;
    fx.put(&place, "arrives.txt", b"hello");
    let change = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let c = changes.recv().await.unwrap();
            if c.path == "arrives.txt" {
                return c;
            }
        }
    })
    .await
    .expect("the watcher reports the new file");
    assert_eq!(change.change, PlaceChangeKind::Created);
    assert!(!change.by_ordin);
    fx.service.unwatch(&[place.id]).await;
    assert!(!fx
        .service
        .place_row(place.id)
        .await
        .unwrap()
        .index_state
        .is_empty());
    fx.service.shutdown().await;
}

#[tokio::test]
async fn versions_restore_current_and_prune() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.md", b"one");
    let v2 = write(&fx, &place, "a.md", b"two").await;
    let v3 = write(&fx, &place, "a.md", b"three").await;
    assert_eq!(
        fx.service
            .current_version(place.id, "a.md")
            .await
            .unwrap()
            .unwrap()
            .id,
        v3.id
    );
    assert!(fx
        .service
        .current_version(place.id, "none.md")
        .await
        .unwrap()
        .is_none());
    let read = fx.service.read_version(v2.id).await.unwrap();
    assert_eq!(
        (read.bytes.as_slice(), read.media_type.as_str()),
        (&b"two"[..], "text/markdown")
    );
    let restored = fx.service.restore_version(v2.id).await.unwrap();
    assert_eq!(restored.version, 4);
    assert_eq!(restored.origin, FileVersionOrigin::Person);
    assert_eq!(std::fs::read(fx.root(&place).join("a.md")).unwrap(), b"two");

    // A held edit keeps its version, the latest is always kept, and unreferenced copies go.
    let future = Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 60_000).unwrap();
    sqlx::query("INSERT INTO held_edits (place_id, path, file_version_id, saved_at) VALUES (?1, 'a.md', ?2, ?3)")
        .bind(place.id.to_string())
        .bind(v3.id.to_string())
        .bind(Timestamp::now().to_wire())
        .execute(fx.pool())
        .await
        .unwrap();
    let report = fx.service.prune_versions(future).await.unwrap();
    assert_eq!(
        report.versions_removed, 2,
        "v1 and v2; v3 is held and v4 is the latest"
    );
    let left = fx.service.versions(place.id, "a.md").await.unwrap();
    assert_eq!(
        left.iter().map(|v| v.version).collect::<Vec<_>>(),
        vec![3, 4]
    );
    // v2's bytes are v4's bytes, so that copy stays; v1's copy went.
    assert!(crate::versions::copy_path(&fx.service.versions_dir, &v2.hash.0).exists());
    assert!(!crate::versions::copy_path(
        &fx.service.versions_dir,
        &crate::versions::hash_of(b"one")
    )
    .exists());
    assert!(report.bytes_freed >= 3);
    assert_eq!(
        fx.service
            .prune_versions(future)
            .await
            .unwrap()
            .versions_removed,
        0
    );
    // Nothing is older than the epoch.
    let epoch = Timestamp::from_unix_millis(0).unwrap();
    assert_eq!(
        fx.service
            .prune_versions(epoch)
            .await
            .unwrap()
            .versions_removed,
        0
    );

    // The Journal's participant calls the same method.
    write(&fx, &place, "a.md", b"five").await;
    sqlx::query("DELETE FROM held_edits")
        .execute(fx.pool())
        .await
        .unwrap();
    let participant = fx.service.prune_participant();
    assert_eq!(participant.module(), "files");
    let removed = participant
        .prune(
            &fx.store,
            &PruneRequest {
                cutoff: future,
                runs_in_progress: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(removed, 2);
    fx.store.close().await;
    let failure = participant
        .prune(
            &fx.store,
            &PruneRequest {
                cutoff: future,
                runs_in_progress: vec![],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "FILE_DATASTORE_UNAVAILABLE");
}

#[tokio::test]
async fn import_copies_files_under_numbered_names_and_refuses_protected_sources() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let drop_dir = fx.dir.0.join("dropped");
    std::fs::create_dir_all(&drop_dir).unwrap();
    std::fs::write(drop_dir.join("a.txt"), b"first").unwrap();
    std::fs::write(drop_dir.join("b.txt"), b"second").unwrap();
    fx.put(&place, "in/a.txt", b"already here");
    let mut changes = fx.service.subscribe_changes();
    let entries = fx
        .service
        .import_files(
            place.id,
            "in",
            vec![drop_dir.join("a.txt"), drop_dir.join("b.txt")],
        )
        .await
        .unwrap();
    assert_eq!(
        entries.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        ["in/a (2).txt", "in/b.txt"]
    );
    assert_eq!(entries[1].size_bytes, 6);
    assert_eq!(entries[1].changed_by.as_deref(), Some("you"));
    let c = next_change(&mut changes).await;
    assert_eq!((c.change, c.by_ordin), (PlaceChangeKind::Created, false));
    assert_eq!(fx.entries("files_imported").await[0].data["count"], 2);
    // Importing the same file again numbers it again.
    let again = fx
        .service
        .import_files(place.id, "in", vec![drop_dir.join("a.txt")])
        .await
        .unwrap();
    assert_eq!(again[0].path, "in/a (3).txt");
    // Refusals: a protected source, a missing source, an agent folder place.
    std::fs::write(fx.protected.join("secret.txt"), b"s").unwrap();
    assert_eq!(
        fx.service
            .import_files(place.id, "", vec![fx.protected.join("secret.txt")])
            .await
            .unwrap_err()
            .code(),
        "FILE_PROTECTED_LOCATION"
    );
    assert_eq!(
        fx.service
            .import_files(place.id, "", vec![drop_dir.join("nope.txt")])
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
    let agent = fx.place("Agent", PlaceKind::AgentFolder).await;
    assert_eq!(
        fx.service
            .import_files(agent.id, "", vec![drop_dir.join("a.txt")])
            .await
            .unwrap_err()
            .code(),
        "FILE_PROTECTED_LOCATION"
    );
}

#[tokio::test]
// spec: ordin-desktop/components/FileService#FS-FETCH-1
async fn fetched_bytes_are_stored_without_a_change_and_the_first_fetch_is_version_1() {
    let fx = fixture().await;
    let copy = fx.place("Copy", PlaceKind::WorkingCopy).await;
    let mut changes = fx.service.subscribe_changes();
    let when = Timestamp::from_unix_millis(1_700_000_000_000).unwrap();
    let hash = fx
        .service
        .store_fetched(
            copy.id,
            "Doc.md",
            b"fetched one".to_vec(),
            Some("Kofi".into()),
            when,
        )
        .await
        .unwrap();
    assert_eq!(hash.0, crate::versions::hash_of(b"fetched one"));
    // FS-FETCH-1: the first fetch is version 1, origin Person, as the working copy item counts it.
    let first = fx.service.versions(copy.id, "Doc.md").await.unwrap();
    assert_eq!(first.len(), 1, "one version row");
    assert_eq!(first[0].version, 1);
    assert_eq!(first[0].origin, FileVersionOrigin::Person);
    assert!(
        no_change(&mut changes, Duration::from_millis(200)).await,
        "no PlaceChange"
    );
    fx.service.rescan_place(copy.id).await.unwrap();
    let listed = fx
        .service
        .list_folder(copy.id, "", FolderSort::NameAsc, None)
        .await
        .unwrap();
    assert_eq!(listed.items[0].changed_by.as_deref(), Some("Kofi"));
    assert_eq!(listed.items[0].changed_at, when);
    // A fetched change is a new version with origin Person.
    write(&fx, &copy, "Doc.md", b"local edit").await;
    fx.service
        .store_fetched(copy.id, "Doc.md", b"fetched two".to_vec(), None, when)
        .await
        .unwrap();
    let versions = fx.service.versions(copy.id, "Doc.md").await.unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[2].origin, FileVersionOrigin::Person);
    // The same bytes again add nothing; other place kinds are refused.
    fx.service
        .store_fetched(copy.id, "Doc.md", b"fetched two".to_vec(), None, when)
        .await
        .unwrap();
    assert_eq!(
        fx.service.versions(copy.id, "Doc.md").await.unwrap().len(),
        3
    );
    let persons = fx.place("P", PlaceKind::PersonsFolder).await;
    assert_eq!(
        fx.service
            .store_fetched(persons.id, "a", vec![], None, when)
            .await
            .unwrap_err()
            .code(),
        "FILE_PROTECTED_LOCATION"
    );
}

#[tokio::test]
async fn places_hide_agent_folders_locate_the_deepest_and_remove_cleanly() {
    let fx = fixture().await;
    let outer = fx.place("Outer", PlaceKind::PersonsFolder).await;
    let agent = fx.place("Agent", PlaceKind::AgentFolder).await;
    let inner_path = fx.root(&outer).join("inner");
    std::fs::create_dir_all(&inner_path).unwrap();
    let inner = fx
        .service
        .add_place(crate::types::AddPlace {
            path: inner_path.clone(),
            friendly_name: Some("Inner".into()),
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap();
    let ids = [outer.id, agent.id, inner.id];
    let visible = fx.service.places(&ids, false).await.unwrap();
    assert_eq!(
        visible
            .iter()
            .map(|p| p.friendly_name.as_str())
            .collect::<Vec<_>>(),
        ["Outer", "Inner"]
    );
    assert_eq!(fx.service.places(&ids, true).await.unwrap().len(), 3);
    assert_eq!(
        fx.service
            .places(&[inner.id, outer.id], false)
            .await
            .unwrap()[0]
            .id,
        inner.id,
        "given order"
    );
    // locate: the deepest place root wins, at a segment boundary.
    assert_eq!(
        fx.service.locate(&inner_path.join("x/y.md")).await.unwrap(),
        Some((inner.id, "x/y.md".to_owned()))
    );
    assert_eq!(
        fx.service
            .locate(&fx.root(&outer).join("z.md"))
            .await
            .unwrap(),
        Some((outer.id, "z.md".to_owned()))
    );
    let sibling = fx.dir.0.join("places").join("Outer-not");
    assert_eq!(
        fx.service.locate(&sibling.join("a.md")).await.unwrap(),
        None
    );
    assert_eq!(
        fx.service
            .locate(&fx.dir.0.join("elsewhere/a.md"))
            .await
            .unwrap(),
        None
    );
    // Renaming works and is journalled; removal leaves the folder and the versions.
    assert_eq!(
        fx.service
            .rename_place(inner.id, "Renamed".into())
            .await
            .unwrap()
            .friendly_name,
        "Renamed"
    );
    fx.put(&outer, "keep.md", b"one");
    write(&fx, &outer, "keep.md", b"two").await;
    fx.service.rescan_place(outer.id).await.unwrap();
    fx.service.remove_place(outer.id).await.unwrap();
    assert!(
        fx.root(&outer).join("keep.md").exists(),
        "the folder is never touched"
    );
    assert_eq!(
        fx.service.place(outer.id).await.unwrap_err().code(),
        "FILE_PLACE_NOT_FOUND"
    );
    assert_eq!(
        fx.service
            .versions(outer.id, "keep.md")
            .await
            .unwrap()
            .len(),
        2,
        "history stays"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_index WHERE place_id = ?1")
        .bind(outer.id.to_string())
        .fetch_one(fx.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0);
    assert_eq!(fx.entries("place_removed").await.len(), 1);
    assert_eq!(fx.entries("place_renamed").await.len(), 1);
}

#[tokio::test]
async fn attribution_indexes_an_unindexed_file_and_refuses_a_missing_one() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::WorkingCopy).await;
    fx.put(&place, "a.md", b"a");
    fx.service
        .set_attribution(place.id, "a.md", "Kofi".into())
        .await
        .unwrap();
    let page = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, None)
        .await
        .unwrap();
    assert_eq!(page.items[0].changed_by.as_deref(), Some("Kofi"));
    assert_eq!(
        fx.service
            .set_attribution(place.id, "none.md", "Kofi".into())
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
}

#[tokio::test]
async fn search_matches_names_text_and_never_passes_syntax_to_the_engine() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let other = fx.place("Q", PlaceKind::PersonsFolder).await;
    fx.put(&place, "100%_done.md", b"percent name");
    fx.put(
        &place,
        "notes.md",
        "# Notes\n\nThe Wärmepumpe quote is ready, \"quoted\" AND OR NOT".as_bytes(),
    );
    fx.put(&other, "other.md", b"quote elsewhere");
    for p in [&place, &other] {
        fx.service.rescan_place(p.id).await.unwrap();
    }
    // `%` and `_` are literal; a name match carries no snippet.
    let hits = fx
        .service
        .search(&[place.id], "100%_", 10)
        .await
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert!(hits[0].name_match && hits[0].snippet.is_none());
    // Prefix on the last word, diacritics folded, case folded, other places left out.
    let hits = fx
        .service
        .search(&[place.id], "warmepum", 10)
        .await
        .unwrap()
        .hits;
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].name_match && hits[0].snippet.is_some());
    assert_eq!(
        fx.service
            .search(&[place.id], "QUOTE", 10)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    assert_eq!(
        fx.service
            .search(&[place.id, other.id], "quote", 10)
            .await
            .unwrap()
            .hits
            .len(),
        2
    );
    // Operators and quotes in the query are plain words.
    for query in ["\"quoted\" AND", "OR NOT", "quote*", "(", "\"", "-x"] {
        fx.service.search(&[place.id], query, 10).await.unwrap();
    }
    assert!(fx
        .service
        .search(&[], "quote", 10)
        .await
        .unwrap()
        .hits
        .is_empty());
    assert_eq!(
        fx.service
            .search(&[place.id], "quote", 1)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
}

#[tokio::test]
async fn published_copies_are_found_by_original_and_by_link_and_record_their_changes() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let link = ConfluenceLinkId::new();
    let page = KnowledgePageId::new();
    let mut on_page = copy_for(CopyOriginal::KnowledgePage {
        knowledge_page_id: page,
    });
    on_page.published_via = PublishedVia::ConfluenceLink {
        confluence_link_id: link,
    };
    let mut on_file = copy_for(CopyOriginal::PlaceFile {
        place_id: place.id,
        path: "a.md".into(),
    });
    on_file.followed_branch = Some("main".into());
    on_file.destination_item_id = Some("item-1".into());
    let on_page = fx.service.record_published_copy(on_page).await.unwrap();
    let on_file = fx.service.record_published_copy(on_file).await.unwrap();
    assert_eq!(
        fx.service.published_copy(on_file.id).await.unwrap(),
        on_file
    );
    assert_eq!(on_file.followed_branch.as_deref(), Some("main"));
    let of_page = fx
        .service
        .published_copies_of(&CopyOriginal::KnowledgePage {
            knowledge_page_id: page,
        })
        .await
        .unwrap();
    assert_eq!(
        of_page.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![on_page.id]
    );
    assert_eq!(
        fx.service
            .published_copies_via_link(link)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(fx
        .service
        .published_copies_via_link(ConfluenceLinkId::new())
        .await
        .unwrap()
        .is_empty());
    // The walk to current stamps last_updated_at; a failure needs its reason and clears it later.
    let c = fx
        .service
        .set_published_copy_state(on_file.id, PublishedCopyState::AwaitingApproval, None)
        .await
        .unwrap();
    assert!(c.last_updated_at.is_none());
    let c = fx
        .service
        .set_published_copy_state(on_file.id, PublishedCopyState::Current, None)
        .await
        .unwrap();
    assert!(c.last_updated_at.is_some());
    fx.service
        .set_published_copy_state(on_file.id, PublishedCopyState::OutOfDate, None)
        .await
        .unwrap();
    fx.service
        .set_published_copy_state(on_file.id, PublishedCopyState::Updating, None)
        .await
        .unwrap();
    let c = fx
        .service
        .set_published_copy_state(
            on_file.id,
            PublishedCopyState::Failed,
            Some("the site said no".into()),
        )
        .await
        .unwrap();
    assert_eq!(c.failure_reason.as_deref(), Some("the site said no"));
    let c = fx
        .service
        .set_published_copy_state(on_file.id, PublishedCopyState::Updating, None)
        .await
        .unwrap();
    assert!(c.failure_reason.is_none());
    assert_eq!(fx.entries("published_copy_recorded").await.len(), 2);
}

fn copy_for(original: CopyOriginal) -> PublishedCopy {
    PublishedCopy {
        id: PublishedCopyId::new(),
        original,
        destination: CopyDestination::ConfluencePage {
            space: "ENG".into(),
            parent_page_id: "1".into(),
        },
        destination_item_id: None,
        format: CopyFormat::Page,
        maker: CopyMaker::Ordin,
        published_via: PublishedVia::PublishACopy,
        followed_branch: None,
        keep_up_to_date: true,
        audience: "the team".into(),
        original_version_published: Some("v1".into()),
        destination_version_pushed: None,
        last_updated_at: None,
        state: PublishedCopyState::ReadyToPublish,
        failure_reason: None,
    }
}

#[tokio::test]
async fn a_persons_folder_edit_is_not_held_and_writes_into_agent_folders_are_refused() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.md", b"a");
    let mut changes = fx.service.subscribe_changes();
    fx.service
        .save_person_edit(place.id, "a.md", b"edited".to_vec())
        .await
        .unwrap();
    assert!(fx.service.held_edits(&[place.id]).await.unwrap().is_empty());
    let c = next_change(&mut changes).await;
    assert!(!c.by_ordin, "the person's own edit");
    let agent = fx.place("Agent", PlaceKind::AgentFolder).await;
    assert_eq!(
        fx.service
            .save_person_edit(agent.id, "a.md", vec![])
            .await
            .unwrap_err()
            .code(),
        "FILE_PROTECTED_LOCATION"
    );
}

#[tokio::test]
async fn reads_for_an_agent_respect_the_limit_missing_files_and_crew_grants() {
    use crate::{FolderAccess, FolderGrant, Grantee};
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.txt", b"0123456789");
    let workspace = ordin_types::WorkspaceId::new();
    let agent = ordin_types::AgentInstanceId::new();
    let crew_only = FolderGrant {
        id: ordin_types::FolderGrantId::new(),
        workspace_id: workspace,
        grantee: Grantee::Crew {
            crew_charter_id: ordin_types::CrewCharterId::new(),
        },
        place_id: place.id,
        access: FolderAccess::Read,
        granted_at: Timestamp::now(),
    };
    fx.service
        .publish_grants(workspace, vec![crew_only.clone()])
        .await
        .unwrap();
    assert_eq!(
        fx.service
            .read_for_agent(agent, workspace, place.id, "a.txt", 100)
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_GRANTED",
        "a crew grant is not the instance's"
    );
    let mine = FolderGrant {
        id: ordin_types::FolderGrantId::new(),
        grantee: Grantee::AgentInstance {
            agent_instance_id: agent,
        },
        ..crew_only
    };
    fx.service
        .publish_grants(workspace, vec![mine])
        .await
        .unwrap();
    assert_eq!(
        fx.service
            .read_for_agent(agent, workspace, place.id, "a.txt", 10)
            .await
            .unwrap(),
        b"0123456789"
    );
    let error = fx
        .service
        .read_for_agent(agent, workspace, place.id, "a.txt", 9)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        crate::FileError::TooLarge { limit_bytes: 9 }
    ));
    assert_eq!(
        fx.service
            .read_for_agent(agent, workspace, place.id, "none.txt", 10)
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
    assert_eq!(
        fx.service
            .read_for_agent(agent, workspace, place.id, "../x", 10)
            .await
            .unwrap_err()
            .code(),
        "FILE_PATH_ESCAPES_PLACE"
    );
    // The limit is lowered to READ_LIMIT_BYTES.
    assert_eq!(
        fx.service
            .read_for_agent(agent, workspace, place.id, "a.txt", u64::MAX)
            .await
            .unwrap()
            .len(),
        10
    );
}

#[tokio::test]
async fn shutdown_is_safe_twice_and_start_resumes_the_watch() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.txt", b"a");
    fx.service.watch(&[place.id]).await.unwrap();
    wait_ready(&fx, &place).await;
    fx.service.shutdown().await;
    fx.service.shutdown().await;
    // A place whose folder vanished while closed is logged and left unwatched, not removed.
    let missing = fx.place("Gone", PlaceKind::PersonsFolder).await;
    sqlx::query("UPDATE places SET watched = 1 WHERE id = ?1")
        .bind(missing.id.to_string())
        .execute(fx.pool())
        .await
        .unwrap();
    std::fs::remove_dir_all(fx.root(&missing)).unwrap();
    fx.service.start().await.unwrap();
    assert!(fx.service.place(missing.id).await.is_ok());
    fx.service.shutdown().await;
}

#[tokio::test]
async fn a_second_service_needs_a_sqlite_datastore_and_hides_nothing_from_the_first() {
    let fx = fixture().await;
    let second = second_service(&fx).await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    assert_eq!(second.place(place.id).await.unwrap().id, place.id);
}

fn run_origin(version: &crate::FileVersion, run: RunId, wanted: &str) -> bool {
    matches!(&version.origin, FileVersionOrigin::Run { run_id, tool } if *run_id == run && tool == wanted)
}

#[tokio::test]
async fn a_harness_write_under_a_claim_is_the_runs() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    let run = RunId::new();
    let path = fx.root(&place).join("rates.md");
    let claim = fx
        .service
        .open_agent_write(run, "Write", AgentWriteTarget::File(path.clone()))
        .await
        .unwrap()
        .expect("a path in a place has a claim");
    assert!(fx
        .service
        .agent_write_open(place.id, "rates.md")
        .await
        .unwrap());
    fx.put(&place, "rates.md", b"written by a harness");
    fx.service.settle_agent_write(&claim).await.unwrap();
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert_eq!(versions.len(), 1);
    assert!(run_origin(&versions[0], run, "Write"));
    assert!(!fx
        .service
        .agent_write_open(place.id, "rates.md")
        .await
        .unwrap());
    // A person's edit afterwards is the person's.
    fx.put(&place, "rates.md", b"then edited by the person");
    fx.service.rescan_place(place.id).await.unwrap();
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert_eq!(versions.last().unwrap().origin, FileVersionOrigin::Person);
}

#[tokio::test]
async fn the_watcher_seeing_a_claimed_write_first_gives_it_to_the_run_once() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    fx.service
        .baseline_pub(place.id, "rates.md", b"first".to_vec())
        .await
        .unwrap();
    fx.service.rescan_place(place.id).await.unwrap();
    let run = RunId::new();
    let path = fx.root(&place).join("rates.md");
    let claim = fx
        .service
        .open_agent_write(run, "Edit", AgentWriteTarget::File(path))
        .await
        .unwrap()
        .unwrap();
    fx.put(&place, "rates.md", b"edited");
    fx.service.rescan_place(place.id).await.unwrap();
    fx.service.settle_agent_write(&claim).await.unwrap();
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert_eq!(versions.len(), 2);
    assert!(run_origin(&versions[1], run, "Edit"));
}

#[tokio::test]
async fn a_change_with_no_claim_stays_the_persons_and_a_failed_call_adds_nothing() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    fx.service
        .baseline_pub(place.id, "rates.md", b"first".to_vec())
        .await
        .unwrap();
    let path = fx.root(&place).join("rates.md");
    let claim = fx
        .service
        .open_agent_write(RunId::new(), "Write", AgentWriteTarget::File(path))
        .await
        .unwrap()
        .unwrap();
    fx.service.settle_agent_write(&claim).await.unwrap();
    assert_eq!(
        fx.service
            .versions(place.id, "rates.md")
            .await
            .unwrap()
            .len(),
        1
    );
    fx.put(&place, "rates.md", b"second");
    fx.service.rescan_place(place.id).await.unwrap();
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert_eq!(versions.last().unwrap().origin, FileVersionOrigin::Person);
}

#[tokio::test]
async fn a_wide_claim_covers_every_place_until_its_run_is_rescanned() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    fx.service
        .baseline_pub(place.id, "rates.md", b"first".to_vec())
        .await
        .unwrap();
    fx.service.rescan_place(place.id).await.unwrap();
    fx.service.watch(&[place.id]).await.unwrap();
    let run = RunId::new();
    for _ in 0..2 {
        let none = fx
            .service
            .open_agent_write(run, "Bash", AgentWriteTarget::Anywhere)
            .await
            .unwrap();
        assert_eq!(none, None);
    }
    assert!(fx
        .service
        .agent_write_open(place.id, "anything.md")
        .await
        .unwrap());
    fx.put(&place, "rates.md", b"from a shell");
    fx.put(&place, "target.o", b"a build output");
    fx.service.agent_run_ended(run).await.unwrap();
    assert!(!fx
        .service
        .agent_write_open(place.id, "rates.md")
        .await
        .unwrap());
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert!(run_origin(versions.last().unwrap(), run, "Bash"));
    // A new file under a wide claim keeps no copy: a build would keep all its output.
    assert!(fx
        .service
        .versions(place.id, "target.o")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_path_outside_every_place_needs_no_claim() {
    let fx = fixture().await;
    let elsewhere = fx.dir.0.join("not-a-place").join("a.md");
    let claim = fx
        .service
        .open_agent_write(RunId::new(), "Write", AgentWriteTarget::File(elsewhere))
        .await
        .unwrap();
    assert_eq!(claim, None);
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tokio::test]
async fn a_claim_covers_the_path_under_any_case() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    let path = fx.root(&place).join("RATES.md");
    fx.service
        .open_agent_write(RunId::new(), "Write", AgentWriteTarget::File(path))
        .await
        .unwrap();
    assert!(fx
        .service
        .agent_write_open(place.id, "rates.md")
        .await
        .unwrap());
}

#[tokio::test]
async fn claims_a_stopped_process_left_are_settled_at_the_next_start() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    let run = RunId::new();
    let path = fx.root(&place).join("rates.md");
    fx.service
        .open_agent_write(run, "Write", AgentWriteTarget::File(path))
        .await
        .unwrap();
    fx.service
        .open_agent_write(run, "Bash", AgentWriteTarget::Anywhere)
        .await
        .unwrap();
    fx.put(&place, "rates.md", b"written just before the crash");
    let next = second_service(&fx).await;
    next.start().await.unwrap();
    let settled = async {
        while next.agent_write_open(place.id, "rates.md").await.unwrap() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), settled)
        .await
        .expect("the claims close");
    let versions = next.versions(place.id, "rates.md").await.unwrap();
    assert!(run_origin(&versions[0], run, "Write"));
    next.shutdown().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_path_that_cannot_be_named_for_certain_takes_a_wide_claim() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    let root = fx.root(&place);
    let elsewhere = fx.dir.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join("sym")).unwrap();
    std::os::unix::fs::symlink("/no/such/target", root.join("dangling")).unwrap();
    for path in [
        root.join("nope/../rates.md"),
        root.join("sym/../rates.md"),
        root.join("dangling"),
    ] {
        let run = RunId::new();
        let none = fx
            .service
            .open_agent_write(run, "Write", AgentWriteTarget::File(path.clone()))
            .await
            .unwrap();
        assert_eq!(none, None, "{}", path.display());
        assert!(
            fx.service
                .agent_write_open(place.id, "rates.md")
                .await
                .unwrap(),
            "{}",
            path.display()
        );
        fx.service.agent_run_ended(run).await.unwrap();
    }
}

#[tokio::test]
async fn a_path_claim_covers_every_place_holding_the_file() {
    let fx = fixture().await;
    let outer = fx.place("Outer", PlaceKind::WorkingCopy).await;
    let inner_root = fx.root(&outer).join("inner");
    std::fs::create_dir_all(&inner_root).unwrap();
    let inner = fx
        .service
        .add_place(AddPlace {
            path: inner_root.clone(),
            friendly_name: Some("Inner".to_owned()),
            kind: PlaceKind::WorkingCopy,
        })
        .await
        .unwrap();
    let run = RunId::new();
    let path = inner_root.join("rates.md");
    let claim = fx
        .service
        .open_agent_write(run, "Write", AgentWriteTarget::File(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert!(fx
        .service
        .agent_write_open(inner.id, "rates.md")
        .await
        .unwrap());
    assert!(fx
        .service
        .agent_write_open(outer.id, "inner/rates.md")
        .await
        .unwrap());
    std::fs::write(&path, "from a harness").unwrap();
    fx.service.settle_agent_write(&claim).await.unwrap();
    for (place, rel) in [(inner.id, "rates.md"), (outer.id, "inner/rates.md")] {
        let versions = fx.service.versions(place, rel).await.unwrap();
        assert!(run_origin(&versions[0], run, "Write"), "{rel}");
        assert!(!fx.service.agent_write_open(place, rel).await.unwrap());
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[tokio::test]
async fn a_claim_finds_its_place_under_any_case() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    let shouted = fx.root(&place).to_string_lossy().to_uppercase();
    let path = std::path::Path::new(&shouted).join("rates.md");
    fx.service
        .open_agent_write(RunId::new(), "Write", AgentWriteTarget::File(path))
        .await
        .unwrap();
    assert!(fx
        .service
        .agent_write_open(place.id, "rates.md")
        .await
        .unwrap());
}

#[tokio::test]
async fn a_wide_claim_stays_on_a_place_no_rescan_covered() {
    let fx = fixture().await;
    let gone = fx.place("Gone", PlaceKind::WorkingCopy).await;
    let here = fx.place("Here", PlaceKind::WorkingCopy).await;
    let moved = fx.dir.0.join("moved-away");
    std::fs::rename(fx.root(&gone), &moved).unwrap();
    let run = RunId::new();
    fx.service
        .open_agent_write(run, "Bash", AgentWriteTarget::Anywhere)
        .await
        .unwrap();
    fx.service.agent_run_ended(run).await.unwrap();
    assert!(fx.service.agent_write_open(gone.id, "a.md").await.unwrap());
    assert!(!fx.service.agent_write_open(here.id, "a.md").await.unwrap());
    // Back again, the next start covers it.
    std::fs::rename(&moved, fx.root(&gone)).unwrap();
    let next = second_service(&fx).await;
    next.start().await.unwrap();
    let closed = async {
        while next.agent_write_open(gone.id, "a.md").await.unwrap() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), closed)
        .await
        .expect("the claim closes");
    next.shutdown().await;
}

#[tokio::test]
async fn a_wide_claim_stays_when_the_service_stops_before_its_rescan() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    let run = RunId::new();
    fx.service
        .open_agent_write(run, "Bash", AgentWriteTarget::Anywhere)
        .await
        .unwrap();
    fx.service
        .stopping
        .store(true, std::sync::atomic::Ordering::SeqCst);
    fx.service.agent_run_ended(run).await.unwrap();
    assert!(fx.service.agent_write_open(place.id, "a.md").await.unwrap());
}

#[tokio::test]
async fn a_wide_claim_sees_a_change_in_a_place_nothing_watches() {
    let fx = fixture().await;
    let place = fx.place("Unwatched", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    fx.service
        .baseline_pub(place.id, "rates.md", b"first".to_vec())
        .await
        .unwrap();
    fx.service.rescan_place(place.id).await.unwrap();
    let run = RunId::new();
    fx.service
        .open_agent_write(run, "Bash", AgentWriteTarget::Anywhere)
        .await
        .unwrap();
    fx.put(&place, "rates.md", b"from a shell");
    fx.service.agent_run_ended(run).await.unwrap();
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert!(run_origin(versions.last().unwrap(), run, "Bash"));
}

#[tokio::test]
async fn a_claim_opened_while_the_bytes_are_read_covers_them() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    fx.put(&place, "rates.md", b"first");
    fx.service
        .baseline_pub(place.id, "rates.md", b"first".to_vec())
        .await
        .unwrap();
    fx.service.rescan_place(place.id).await.unwrap();
    fx.put(&place, "rates.md", b"changed");
    let run = RunId::new();
    let service = fx.service.clone();
    let path = fx.root(&place).join("rates.md");
    *fx.service.after_read.lock().unwrap() = Some(Box::pin(async move {
        service
            .open_agent_write(run, "Write", AgentWriteTarget::File(path))
            .await
            .unwrap();
    }));
    fx.service.rescan_place(place.id).await.unwrap();
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert!(
        run_origin(versions.last().unwrap(), run, "Write"),
        "{:?}",
        versions.last().unwrap().origin
    );
}

#[tokio::test]
async fn pruning_keeps_what_the_authorship_check_reads() {
    let fx = fixture().await;
    let place = fx.place("Linked", PlaceKind::WorkingCopy).await;
    write(&fx, &place, "rates.md", b"old").await;
    let published = write(&fx, &place, "rates.md", b"published").await;
    let run = RunId::new();
    fx.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "rates.md".to_owned(),
            bytes: b"an agent's text".to_vec(),
            origin: FileVersionOrigin::Run {
                run_id: run,
                tool: "write_file".to_owned(),
            },
            expected_hash: None,
        })
        .await
        .unwrap();
    write(&fx, &place, "rates.md", b"a person's tidy").await;
    let mut copy = copy_for(CopyOriginal::PlaceFile {
        place_id: place.id,
        path: "rates.md".to_owned(),
    });
    copy.state = PublishedCopyState::Current;
    copy.original_version_published = Some(published.hash.0.clone());
    fx.service.record_published_copy(copy).await.unwrap();

    let future = Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 86_400_000).unwrap();
    let report = fx.service.prune_versions(future).await.unwrap();

    assert_eq!(
        report.versions_removed, 1,
        "only the version before it goes"
    );
    let versions = fx.service.versions(place.id, "rates.md").await.unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[0].hash, published.hash);
    assert!(run_origin(&versions[1], run, "write_file"));
}

// Unix only: a file is made unreadable with mode bits, which Windows does not have.
#[cfg(unix)]
#[tokio::test]
async fn an_unreadable_file_does_not_end_a_rescan() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture().await;
    let place = fx.place("Locked", PlaceKind::WorkingCopy).await;
    let locked = fx.put(&place, "locked.md", b"first");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    fx.put(&place, "later.md", b"second");

    let scanned = fx.service.rescan_place(place.id).await;

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    scanned.expect("the rescan goes on past a file it cannot read");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_racing_a_scan_lands() {
    let fx = fixture().await;
    let place = fx.place("Race", PlaceKind::WorkingCopy).await;
    for n in 0..20 {
        fx.put(&place, &format!("{n}.md"), b"first");
    }
    let scanner = fx.service.clone();
    let scan = tokio::spawn(async move {
        for _ in 0..10 {
            scanner.rescan_place(place.id).await?;
        }
        Ok::<_, crate::FileError>(())
    });
    for round in 0..60 {
        fx.service
            .write_version(WriteVersion {
                place_id: place.id,
                path: format!("{}.md", round % 20),
                bytes: format!("round {round}").into_bytes(),
                origin: FileVersionOrigin::Person,
                expected_hash: None,
            })
            .await
            .expect("the write lands");
    }
    scan.await.unwrap().expect("the scan lands");
}

/// FU-082: a version insert reads the next number and then writes. In a deferred transaction that is
/// an upgrade from reader to writer, which SQLite refuses at once if another writer committed between
/// the two, whatever the busy timeout, so a write racing a scan failed `FILE_DATASTORE_UNAVAILABLE`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_never_fail_on_the_datastore() {
    let fx = fixture().await;
    let place = fx.place("Race", PlaceKind::WorkingCopy).await;
    let mut tasks = Vec::new();
    for writer in 0..6 {
        let service = fx.service.clone();
        tasks.push(tokio::spawn(async move {
            for round in 0..25 {
                service
                    .write_version(WriteVersion {
                        place_id: place.id,
                        path: format!("{writer}.md"),
                        bytes: format!("{writer} {round}").into_bytes(),
                        origin: FileVersionOrigin::Person,
                        expected_hash: None,
                    })
                    .await?;
            }
            Ok::<_, crate::FileError>(())
        }));
    }
    for task in tasks {
        task.await
            .expect("the writer ends")
            .expect("every write lands");
    }
}
