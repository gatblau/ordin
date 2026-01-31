//! X2 for the moving-a-file amendment (ADR-104), and the rules of Internal Logic 15 and 5a.

use std::time::Duration;

use ordin_types::PublishedCopyId;

use super::support::*;
use crate::types::{PlaceChangeKind, WriteVersion};
use crate::{
    CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileVersionOrigin, PlaceKind,
    PublishedCopy, PublishedCopyState, PublishedVia,
};

#[tokio::test]
async fn happy_path_the_history_follows_the_file() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let from = "knowledge/Supplier rates.md";
    let to = "knowledge/Supplier day rates.md";
    fx.put(&place, from, b"v1 text");
    write(&fx, &place, from, b"v2 text").await;
    write(&fx, &place, from, b"v3 text").await;
    // A published copy of the file and a held edit follow it too.
    let mut copy = copy_of(CopyOriginal::PlaceFile {
        place_id: place.id,
        path: from.to_owned(),
    });
    copy.state = PublishedCopyState::Current;
    let copy = fx.service.record_published_copy(copy).await.unwrap();
    let latest = fx.service.latest(place.id, from).await.unwrap().unwrap();
    sqlx::query("INSERT INTO held_edits (place_id, path, file_version_id, saved_at) VALUES (?1, ?2, ?3, ?4)")
        .bind(place.id.to_string())
        .bind(from)
        .bind(latest.id.to_string())
        .bind(latest.created_at.to_wire())
        .execute(fx.pool())
        .await
        .unwrap();
    let mut changes = fx.service.subscribe_changes();

    let moved = fx.service.move_file(place.id, from, to).await.unwrap();

    assert_eq!((moved.version, moved.path.as_str()), (3, to));
    let at_new = fx.service.versions(place.id, to).await.unwrap();
    assert_eq!(
        at_new.iter().map(|v| v.version).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(fx
        .service
        .versions(place.id, from)
        .await
        .unwrap()
        .is_empty());
    let change = next_change(&mut changes).await;
    assert_eq!(change.path, to);
    assert_eq!(
        change.change,
        PlaceChangeKind::Renamed {
            from: from.to_owned()
        }
    );
    assert!(change.by_ordin);
    assert_eq!(change.version, Some(3));
    assert!(
        no_change(&mut changes, Duration::from_millis(300)).await,
        "announced once"
    );
    assert!(!fx.root(&place).join(from).exists());
    assert_eq!(std::fs::read(fx.root(&place).join(to)).unwrap(), b"v3 text");
    // The index row, the held edit and the copy's key moved with it.
    let indexed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM file_index WHERE place_id = ?1 AND path = ?2")
            .bind(place.id.to_string())
            .bind(to)
            .fetch_one(fx.pool())
            .await
            .unwrap();
    assert_eq!(indexed, 1);
    let held = fx.service.held_edits(&[place.id]).await.unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].path, to);
    let copies = fx
        .service
        .published_copies_of(&CopyOriginal::PlaceFile {
            place_id: place.id,
            path: to.to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].id, copy.id);
    assert!(matches!(&copies[0].original, CopyOriginal::PlaceFile { path, .. } if path == to));
    let old_copies = fx
        .service
        .published_copies_of(&CopyOriginal::PlaceFile {
            place_id: place.id,
            path: from.to_owned(),
        })
        .await
        .unwrap();
    assert!(old_copies.is_empty());
    let journal = fx.entries("file_moved").await;
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].data["versions"], 3);
}

#[tokio::test]
async fn edge_only_the_letter_case_changes() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    fx.put(&place, "knowledge/rates.md", b"one");
    write(&fx, &place, "knowledge/rates.md", b"two").await;
    fx.service
        .move_file(place.id, "knowledge/rates.md", "knowledge/Rates.md")
        .await
        .unwrap();
    let names: Vec<String> = std::fs::read_dir(fx.root(&place).join("knowledge"))
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    assert_eq!(
        names,
        vec!["Rates.md".to_owned()],
        "the file is named Rates.md"
    );
    let versions = fx
        .service
        .versions(place.id, "knowledge/Rates.md")
        .await
        .unwrap();
    assert_eq!(versions.len(), 2);
    assert!(fx
        .service
        .versions(place.id, "knowledge/rates.md")
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn error_the_target_exists() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let a = fx.put(&place, "knowledge/Supplier rates.md", b"rates");
    let b = fx.put(&place, "knowledge/Day rates.md", b"day");
    let error = fx
        .service
        .move_file(
            place.id,
            "knowledge/Supplier rates.md",
            "knowledge/Day rates.md",
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_MOVE_TARGET_EXISTS");
    assert_eq!(std::fs::read(&a).unwrap(), b"rates");
    assert_eq!(std::fs::read(&b).unwrap(), b"day");
}

#[tokio::test]
async fn error_not_a_workspace_folder() {
    let fx = fixture().await;
    let place = fx.place("Q", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.md", b"a");
    let error = fx
        .service
        .move_file(place.id, "a.md", "b.md")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PROTECTED_LOCATION");
    assert!(fx.root(&place).join("a.md").exists());
}

#[tokio::test]
async fn edge_another_app_renames_a_file() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let a = fx.put(&place, "knowledge/a.md", b"first");
    write(&fx, &place, "knowledge/a.md", b"second").await;
    let mut changes = fx.service.subscribe_changes();
    std::fs::rename(&a, fx.root(&place).join("knowledge/b.md")).unwrap();

    fx.service
        .process_batch(
            place.id,
            vec!["knowledge/a.md".into(), "knowledge/b.md".into()],
        )
        .await
        .unwrap();

    let versions = fx
        .service
        .versions(place.id, "knowledge/b.md")
        .await
        .unwrap();
    assert_eq!(
        versions.iter().map(|v| v.version).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(fx
        .service
        .versions(place.id, "knowledge/a.md")
        .await
        .unwrap()
        .is_empty());
    let change = next_change(&mut changes).await;
    assert_eq!(change.path, "knowledge/b.md");
    assert_eq!(
        change.change,
        PlaceChangeKind::Renamed {
            from: "knowledge/a.md".to_owned()
        }
    );
    assert!(!change.by_ordin);
    assert!(no_change(&mut changes, Duration::from_millis(300)).await);
}

#[tokio::test]
async fn a_rename_onto_a_path_with_its_own_history_keeps_both_histories() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let a = fx.put(&place, "a.md", b"a one");
    write(&fx, &place, "a.md", b"a two").await;
    let b = fx.put(&place, "b.md", b"b one");
    write(&fx, &place, "b.md", b"b two").await;
    // b.md is deleted outside ordin; its history stays.
    std::fs::remove_file(&b).unwrap();
    fx.service
        .remove_index_path(place.id, "b.md")
        .await
        .unwrap();
    let mut changes = fx.service.subscribe_changes();
    std::fs::rename(&a, &b).unwrap();
    fx.service
        .process_batch(place.id, vec!["a.md".into(), "b.md".into()])
        .await
        .unwrap();
    let change = next_change(&mut changes).await;
    assert_eq!(
        change.change,
        PlaceChangeKind::Renamed {
            from: "a.md".to_owned()
        }
    );
    // Neither history moved. The new content at b.md is the person's change to that path, so it
    // is kept as a third version there (Internal Logic 5.5).
    let kept_a = fx.service.versions(place.id, "a.md").await.unwrap();
    assert_eq!(kept_a.len(), 2);
    let kept_b = fx.service.versions(place.id, "b.md").await.unwrap();
    assert_eq!(kept_b.len(), 3);
    assert_eq!(kept_b[2].origin, FileVersionOrigin::Person);
}

#[tokio::test]
async fn a_move_onto_a_path_whose_versions_exist_is_refused_and_nothing_moves() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let a = fx.put(&place, "a.md", b"a one");
    let b = fx.put(&place, "b.md", b"b one");
    write(&fx, &place, "b.md", b"b two").await;
    std::fs::remove_file(&b).unwrap();
    let error = fx
        .service
        .move_file(place.id, "a.md", "b.md")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_MOVE_TARGET_EXISTS");
    assert!(a.exists() && !b.exists());
}

#[tokio::test]
async fn a_move_in_the_database_that_collides_renames_the_file_back() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let a = fx.put(&place, "a.md", b"a one");
    // A version row appears at the target between the pre-check and the transaction.
    sqlx::query(
        "CREATE TRIGGER collide BEFORE UPDATE ON file_versions BEGIN \
         INSERT INTO file_versions (id, place_id, path, version, hash, origin, created_at) \
         VALUES ('x', NEW.place_id, NEW.path, NEW.version, NEW.hash, NEW.origin, NEW.created_at); END",
    )
    .execute(fx.pool())
    .await
    .unwrap();
    let error = fx
        .service
        .move_file(place.id, "a.md", "b.md")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_MOVE_TARGET_EXISTS");
    assert!(a.exists(), "the file was renamed back");
    assert!(!fx.root(&place).join("b.md").exists());
}

#[tokio::test]
async fn move_file_edge_cases() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    fx.put(&place, "a.md", b"a");
    // The same path: nothing is done, the latest version comes back (a baseline if none).
    let same = fx
        .service
        .move_file(place.id, "a.md", "./a.md")
        .await
        .unwrap();
    assert_eq!((same.version, same.path.as_str()), (1, "a.md"));
    assert_eq!(
        fx.service
            .move_file(place.id, "gone.md", "b.md")
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
    assert_eq!(
        fx.service
            .move_file(place.id, "a.md", "../b.md")
            .await
            .unwrap_err()
            .code(),
        "FILE_PATH_ESCAPES_PLACE"
    );
    assert_eq!(
        fx.service
            .move_file(crate::PlaceId::new(), "a.md", "b.md")
            .await
            .unwrap_err()
            .code(),
        "FILE_PLACE_NOT_FOUND"
    );
    // A move into a folder that does not exist yet creates it; a baseline is kept first.
    let moved = fx
        .service
        .move_file(place.id, "a.md", "new/folder/a.md")
        .await
        .unwrap();
    assert_eq!(moved.version, 1);
    assert!(fx.root(&place).join("new/folder/a.md").exists());
}

async fn write(fx: &Fixture, place: &crate::Place, path: &str, bytes: &[u8]) {
    fx.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: path.to_owned(),
            bytes: bytes.to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap();
}

fn copy_of(original: CopyOriginal) -> PublishedCopy {
    PublishedCopy {
        id: PublishedCopyId::new(),
        original,
        destination: CopyDestination::ConfluencePage {
            space: "ENG".to_owned(),
            parent_page_id: "1".to_owned(),
        },
        destination_item_id: None,
        format: CopyFormat::Page,
        maker: CopyMaker::Ordin,
        published_via: PublishedVia::PublishACopy,
        followed_branch: None,
        keep_up_to_date: true,
        audience: "the team".to_owned(),
        original_version_published: None,
        destination_version_pushed: None,
        last_updated_at: None,
        state: PublishedCopyState::ReadyToPublish,
        failure_reason: None,
    }
}
