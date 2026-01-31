//! X1: one test per row of FileService.md's Error Table (and the row the ADR-104 amendment adds),
//! and X4: the codes this crate owns.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::paging::Cursor;
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, FolderGrantId, PlaceId, PublishedCopyId, RepairAction, WorkspaceId};

use super::support::*;
use crate::types::{AddPlace, FolderSort, WriteVersion};
use crate::{
    AgentRefusal, CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileError,
    FileVersionOrigin, FolderAccess, FolderGrant, Grantee, PlaceKind, PublishedCopy,
    PublishedCopyState, PublishedVia, ALL_CODES,
};

fn person_write(place: &crate::Place, path: &str, bytes: &[u8]) -> WriteVersion {
    WriteVersion {
        place_id: place.id,
        path: path.to_owned(),
        bytes: bytes.to_vec(),
        origin: FileVersionOrigin::Person,
        expected_hash: None,
    }
}

fn a_copy() -> PublishedCopy {
    PublishedCopy {
        id: PublishedCopyId::new(),
        original: CopyOriginal::PlaceFile {
            place_id: PlaceId::new(),
            path: "a.md".to_owned(),
        },
        destination: CopyDestination::ConfluenceAttachment {
            space: "ENG".to_owned(),
            parent_page_id: "9".to_owned(),
        },
        destination_item_id: None,
        format: CopyFormat::ExactFile,
        maker: CopyMaker::None,
        published_via: PublishedVia::PublishACopy,
        followed_branch: None,
        keep_up_to_date: false,
        audience: "the team".to_owned(),
        original_version_published: None,
        destination_version_pushed: None,
        last_updated_at: None,
        state: PublishedCopyState::ReadyToPublish,
        failure_reason: None,
    }
}

#[tokio::test]
async fn place_id_unknown() {
    let fx = fixture().await;
    let missing = PlaceId::new();
    for error in [
        fx.service.place(missing).await.unwrap_err(),
        fx.service
            .rename_place(missing, "x".into())
            .await
            .unwrap_err(),
        fx.service.remove_place(missing).await.unwrap_err(),
        fx.service
            .list_folder(missing, "", FolderSort::NameAsc, None)
            .await
            .unwrap_err(),
        fx.service.read_file(missing, "a.md").await.unwrap_err(),
    ] {
        assert_eq!(error.code(), "FILE_PLACE_NOT_FOUND");
        assert!(!error.retryable());
    }
    // `places` skips an unknown id instead.
    assert!(fx
        .service
        .places(&[missing], true)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn file_missing_at_the_path() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let error = fx.service.read_file(place.id, "gone.md").await.unwrap_err();
    assert_eq!(error.code(), "FILE_NOT_FOUND");
    assert!(!error.retryable());
    assert_eq!(
        fx.service
            .preview(place.id, "gone.md")
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
    assert_eq!(
        fx.service
            .move_to_bin(place.id, "gone.md")
            .await
            .unwrap_err()
            .code(),
        "FILE_NOT_FOUND"
    );
}

#[tokio::test]
async fn folder_is_a_protected_system_location() {
    let fx = fixture().await;
    let error = fx
        .service
        .add_place(AddPlace {
            path: fx.protected.join("inside"),
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PROTECTED_LOCATION");
    assert!(!error.retryable());
    // The application's own data root is protected too.
    let error = fx
        .service
        .add_place(AddPlace {
            path: fx.data_root.clone(),
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PROTECTED_LOCATION");
}

#[tokio::test]
async fn a_working_copy_place_under_the_data_root_is_not_a_protected_location() {
    // The Workspace Service keeps every working copy under the application data root, which the
    // Rules Engine protects; the place is added all the same, as a coding task's worktree is.
    let fx = fixture().await;
    let folder = fx.data_root.join("working-copies").join("link");
    std::fs::create_dir_all(&folder).unwrap();
    let place = fx
        .service
        .add_place(AddPlace {
            path: folder,
            friendly_name: None,
            kind: PlaceKind::WorkingCopy,
        })
        .await
        .expect("a working copy under the data root is added");
    assert_eq!(place.kind, PlaceKind::WorkingCopy);
    // Another kind of place at the same root is still refused.
    let other = fx.data_root.join("working-copies").join("other");
    std::fs::create_dir_all(&other).unwrap();
    let error = fx
        .service
        .add_place(AddPlace {
            path: other,
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PROTECTED_LOCATION");
}

// Unix only: a folder is made unreadable with mode bits, which Windows does not have.
#[cfg(unix)]
#[tokio::test]
async fn folder_cannot_be_read_by_the_person_s_account() {
    let fx = fixture().await;
    let folder = fx.dir.0.join("places/closed");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000)).unwrap();
    let error = fx
        .service
        .add_place(AddPlace {
            path: folder,
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_FOLDER_UNREADABLE");
    assert!(!error.retryable());
    // A path that does not exist, and a file, are unreadable folders too.
    let missing = fx.dir.0.join("places/none");
    let file = fx.dir.0.join("a-file");
    std::fs::write(&file, b"x").unwrap();
    for path in [missing, file] {
        let error = fx
            .service
            .add_place(AddPlace {
                path,
                friendly_name: None,
                kind: PlaceKind::PersonsFolder,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "FILE_FOLDER_UNREADABLE");
    }
}

#[tokio::test]
async fn folder_already_a_place() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let error = fx
        .service
        .add_place(AddPlace {
            path: fx.root(&place),
            friendly_name: Some("Again".into()),
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PLACE_EXISTS");
    assert!(!error.retryable());
    assert!(matches!(error, FileError::PlaceExists { existing } if existing == place.id));
}

#[tokio::test]
async fn friendly_name_empty_too_long_or_with_control_characters() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    for name in ["", "   ", &"x".repeat(121), "bad\u{7}name"] {
        let error = fx
            .service
            .rename_place(place.id, name.to_owned())
            .await
            .unwrap_err();
        assert_eq!(error.code(), "PLACE_NAME_INVALID", "{name:?}");
        assert!(!error.retryable());
    }
    let folder = fx.dir.0.join("places/Other");
    std::fs::create_dir_all(&folder).unwrap();
    let error = fx
        .service
        .add_place(AddPlace {
            path: folder,
            friendly_name: Some(String::new()),
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PLACE_NAME_INVALID");
    // 120 characters is the limit and is accepted; the name is trimmed.
    let renamed = fx
        .service
        .rename_place(place.id, format!("  {}  ", "x".repeat(120)))
        .await
        .unwrap();
    assert_eq!(renamed.friendly_name.chars().count(), 120);
}

#[tokio::test]
async fn path_not_absolute_or_over_4096_bytes() {
    let fx = fixture().await;
    for path in [
        std::path::PathBuf::from("relative/folder"),
        std::path::PathBuf::from(format!("/{}", "a".repeat(4096))),
    ] {
        let error = fx
            .service
            .add_place(AddPlace {
                path,
                friendly_name: None,
                kind: PlaceKind::PersonsFolder,
            })
            .await
            .unwrap_err();
        assert_eq!(error.code(), "PLACE_PATH_INVALID");
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn relative_path_escapes_its_place() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let outside = fx.dir.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
    // Unix only: a symbolic link needs a privilege on Windows.
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, fx.root(&place).join("link")).unwrap();
    let paths = ["../outside/secret.txt", "a/../../x", "a\0b"];
    let link = cfg!(unix).then_some("link/secret.txt");
    for path in paths.into_iter().chain(link) {
        let error = fx.service.read_file(place.id, path).await.unwrap_err();
        assert_eq!(error.code(), "FILE_PATH_ESCAPES_PLACE", "{path:?}");
        assert!(!error.retryable());
    }
    #[cfg(unix)]
    {
        let error = fx
            .service
            .write_version(person_write(&place, "link/new.md", b"x"))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "FILE_PATH_ESCAPES_PLACE");
        assert!(!outside.join("new.md").exists());
    }
}

#[tokio::test]
async fn read_or_write_above_64_mib() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let big = fx.put(&place, "big.bin", b"");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&big)
        .unwrap()
        .set_len(crate::READ_LIMIT_BYTES + 1)
        .unwrap();
    let error = fx.service.read_file(place.id, "big.bin").await.unwrap_err();
    assert_eq!(error.code(), "FILE_TOO_LARGE");
    assert!(!error.retryable());
    assert!(
        matches!(error, FileError::TooLarge { limit_bytes } if limit_bytes == crate::READ_LIMIT_BYTES)
    );
    let bytes = vec![0u8; (crate::READ_LIMIT_BYTES + 1) as usize];
    let error = fx
        .service
        .write_version(person_write(&place, "huge.bin", &bytes))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_TOO_LARGE");
    assert!(!fx.root(&place).join("huge.bin").exists());
}

#[tokio::test]
async fn expected_hash_differs_from_the_file_on_disk() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.md", b"now");
    let mut write = person_write(&place, "a.md", b"later");
    write.expected_hash = Some(crate::Sha256Hex(crate::versions::hash_of(b"then")));
    let error = fx.service.write_version(write).await.unwrap_err();
    assert_eq!(error.code(), "FILE_CHANGED_MEANWHILE");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    // A file that is not there has no hash to compare.
    let mut write = person_write(&place, "none.md", b"later");
    write.expected_hash = Some(crate::Sha256Hex(crate::versions::hash_of(b"then")));
    assert_eq!(
        fx.service.write_version(write).await.unwrap_err().code(),
        "FILE_NOT_FOUND"
    );
}

// Unix only: the place is made read-only with mode bits, which Windows does not have.
#[cfg(unix)]
#[tokio::test]
async fn disk_write_or_rename_fails() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let root = fx.root(&place);
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let error = fx
        .service
        .write_version(person_write(&place, "new.md", b"x"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_WRITE_FAILED");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        fx.service
            .versions(place.id, "new.md")
            .await
            .unwrap()
            .is_empty(),
        "no version row"
    );
    // The version store cannot be made: the program stops with Diagnostics.
    let blocked = fx.dir.0.join("blocked");
    std::fs::write(&blocked, b"a file where a folder must go").unwrap();
    let error = crate::FileService::new(crate::FileServiceDeps {
        journal: fx.journal.clone(),
        rules: fx.rules.clone(),
        agent_folders: fx.agent_folders.clone(),
        data_root: blocked,
    })
    .await
    .err()
    .expect("the service is not built");
    assert_eq!(error.code(), "FILE_WRITE_FAILED");
}

#[tokio::test]
async fn version_number_gap_or_concurrent_insert_after_one_retry() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.service
        .write_version(person_write(&place, "a.md", b"one"))
        .await
        .unwrap();
    // A concurrent writer takes every number this writer picks.
    sqlx::query(
        "CREATE TRIGGER rival BEFORE INSERT ON file_versions WHEN NEW.id NOT LIKE 'rival-%' BEGIN \
         INSERT INTO file_versions (id, place_id, path, version, hash, origin, created_at) \
         VALUES ('rival-' || NEW.id, NEW.place_id, NEW.path, NEW.version, NEW.hash, NEW.origin, \
                 NEW.created_at); END",
    )
    .execute(fx.pool())
    .await
    .unwrap();
    let error = fx
        .service
        .write_version(person_write(&place, "a.md", b"two"))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FVER_VERSION_GAP");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    assert_eq!(
        fx.service.versions(place.id, "a.md").await.unwrap().len(),
        1,
        "nothing was inserted"
    );
}

#[tokio::test]
async fn prune_would_remove_a_path_s_latest_version() {
    use crate::versions::{ensure_not_latest, Candidate};
    let mut latest = std::collections::HashMap::new();
    latest.insert(("p".to_owned(), "a.md".to_owned()), 3_i64);
    let older = Candidate {
        id: "1".into(),
        place_id: "p".into(),
        path: "a.md".into(),
        version: 2,
        hash: "h".into(),
    };
    assert!(ensure_not_latest(&[older], &latest).is_ok());
    let newest = Candidate {
        id: "2".into(),
        place_id: "p".into(),
        path: "a.md".into(),
        version: 3,
        hash: "h".into(),
    };
    let error = ensure_not_latest(&[newest], &latest).unwrap_err();
    assert_eq!(error.code(), "FVER_LATEST_PRUNE");
    assert!(!error.retryable());
}

#[tokio::test]
async fn system_bin_unavailable() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "a.md", b"x");
    *fx.bin.available.lock().unwrap() = false;
    let error = fx.service.move_to_bin(place.id, "a.md").await.unwrap_err();
    assert_eq!(error.code(), "FILE_BIN_UNAVAILABLE");
    assert!(!error.retryable());
}

#[tokio::test]
async fn version_id_unknown() {
    let fx = fixture().await;
    let id = ordin_types::FileVersionId::new();
    for error in [
        fx.service.read_version(id).await.unwrap_err(),
        fx.service.restore_version(id).await.unwrap_err(),
    ] {
        assert_eq!(error.code(), "FILE_VERSION_NOT_FOUND");
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn version_copy_file_missing_under_the_data_root() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let version = fx
        .service
        .write_version(person_write(&place, "a.md", b"copy me"))
        .await
        .unwrap();
    std::fs::remove_file(crate::versions::copy_path(
        &fx.service.versions_dir,
        &version.hash.0,
    ))
    .unwrap();
    let error = fx.service.read_version(version.id).await.unwrap_err();
    assert_eq!(error.code(), "FILE_VERSION_COPY_MISSING");
    assert!(!error.retryable());
    assert_eq!(error.repair(), None);
    assert_eq!(
        fx.service
            .restore_version(version.id)
            .await
            .unwrap_err()
            .code(),
        "FILE_VERSION_COPY_MISSING"
    );
}

#[tokio::test]
async fn published_copy_id_unknown() {
    let fx = fixture().await;
    let id = PublishedCopyId::new();
    assert_eq!(
        fx.service.published_copy(id).await.unwrap_err().code(),
        "FILE_COPY_NOT_FOUND"
    );
    let error = fx
        .service
        .set_published_copy_state(id, PublishedCopyState::Removed, None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_COPY_NOT_FOUND");
    assert!(!error.retryable());
}

#[tokio::test]
async fn published_copy_state_change_not_in_the_state_table() {
    let fx = fixture().await;
    let copy = fx.service.record_published_copy(a_copy()).await.unwrap();
    let error = fx
        .service
        .set_published_copy_state(copy.id, PublishedCopyState::Updating, None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_COPY_TRANSITION_INVALID");
    assert!(!error.retryable());
    // A replacement row is checked the same way.
    let mut again = copy.clone();
    again.state = PublishedCopyState::Current;
    assert_eq!(
        fx.service
            .record_published_copy(again)
            .await
            .unwrap_err()
            .code(),
        "FILE_COPY_TRANSITION_INVALID"
    );
    // The listed moves are allowed.
    for (from, to) in [
        (
            PublishedCopyState::ReadyToPublish,
            PublishedCopyState::AwaitingApproval,
        ),
        (
            PublishedCopyState::AwaitingApproval,
            PublishedCopyState::Current,
        ),
        (
            PublishedCopyState::AwaitingApproval,
            PublishedCopyState::Failed,
        ),
        (PublishedCopyState::Current, PublishedCopyState::OutOfDate),
        (PublishedCopyState::OutOfDate, PublishedCopyState::Updating),
        (PublishedCopyState::Updating, PublishedCopyState::Failed),
        (PublishedCopyState::Failed, PublishedCopyState::Updating),
        (PublishedCopyState::Updating, PublishedCopyState::Current),
        (
            PublishedCopyState::Current,
            PublishedCopyState::ChangedAtDestination,
        ),
        (
            PublishedCopyState::ChangedAtDestination,
            PublishedCopyState::Current,
        ),
        (
            PublishedCopyState::Current,
            PublishedCopyState::NotKeptUpToDate,
        ),
        (
            PublishedCopyState::NotKeptUpToDate,
            PublishedCopyState::OutOfDate,
        ),
        (PublishedCopyState::OutOfDate, PublishedCopyState::Removed),
    ] {
        assert!(
            crate::copies::transition_allowed(from, to),
            "{from:?} -> {to:?}"
        );
    }
    for (from, to) in [
        (
            PublishedCopyState::Removed,
            PublishedCopyState::NotKeptUpToDate,
        ),
        (PublishedCopyState::Removed, PublishedCopyState::Current),
        (PublishedCopyState::Current, PublishedCopyState::Updating),
        (PublishedCopyState::Failed, PublishedCopyState::Current),
    ] {
        assert!(
            !crate::copies::transition_allowed(from, to),
            "{from:?} -> {to:?}"
        );
    }
}

#[tokio::test]
async fn published_copy_fields_break_a_2c_rule() {
    let fx = fixture().await;
    // exact_file only with confluence_attachment and maker = none.
    let mut copy = a_copy();
    copy.maker = CopyMaker::WordCraft;
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_FORMAT_MAKER_MISMATCH"
    );
    let mut copy = a_copy();
    copy.destination = CopyDestination::ConfluencePage {
        space: "ENG".into(),
        parent_page_id: "9".into(),
    };
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_FORMAT_MAKER_MISMATCH"
    );
    // An exact file to a SharePoint folder is recorded (2C, amended January 2026 by the user's
    // decision).
    // spec: ordin-desktop/phase-2c-shared-types-domain#PUB_FORMAT_MAKER_MISMATCH
    let mut copy = a_copy();
    copy.destination = CopyDestination::SharepointFile {
        site: "s".into(),
        library: "l".into(),
        folder: "f".into(),
    };
    fx.service
        .record_published_copy(copy)
        .await
        .expect("an exact file to a SharePoint folder");
    // page only with confluence_page.
    let mut copy = a_copy();
    copy.format = CopyFormat::Page;
    copy.maker = CopyMaker::Ordin;
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_FORMAT_DESTINATION_MISMATCH"
    );
    // followed_branch only for a repository original.
    let mut copy = a_copy();
    copy.original = CopyOriginal::KnowledgePage {
        knowledge_page_id: ordin_types::KnowledgePageId::new(),
    };
    copy.followed_branch = Some("main".into());
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_BRANCH_UNEXPECTED"
    );
    // failure_reason present if and only if state = failed.
    let mut copy = a_copy();
    copy.failure_reason = Some("it broke".into());
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_FAILURE_REASON_MISMATCH"
    );
    let mut copy = a_copy();
    copy.state = PublishedCopyState::Failed;
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_FAILURE_REASON_MISMATCH"
    );
    let mut copy = a_copy();
    copy.state = PublishedCopyState::Failed;
    copy.failure_reason = Some("x".repeat(501));
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_FAILURE_REASON_MISMATCH"
    );
    // The same rule on a state change.
    let stored = fx.service.record_published_copy(a_copy()).await.unwrap();
    let mut updating = stored.clone();
    updating.state = PublishedCopyState::AwaitingApproval;
    let stored = fx.service.record_published_copy(updating).await.unwrap();
    let error = fx
        .service
        .set_published_copy_state(stored.id, PublishedCopyState::Current, Some("why".into()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "PUB_FAILURE_REASON_MISMATCH");
    assert!(!error.retryable());
    // audience 1–500 characters.
    let mut copy = a_copy();
    copy.audience = String::new();
    assert_eq!(
        fx.service
            .record_published_copy(copy)
            .await
            .unwrap_err()
            .code(),
        "PUB_AUDIENCE_INVALID"
    );
}

#[tokio::test]
async fn search_query_empty_or_over_200_chars_and_a_folder_given_to_import_files() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    for (query, limit) in [
        ("", 10),
        ("   ", 10),
        (&"q".repeat(201), 10),
        ("ok", 0),
        ("ok", 201),
    ] {
        let error = fx
            .service
            .search(&[place.id], query, limit)
            .await
            .unwrap_err();
        assert_eq!(error.code(), "FILE_QUERY_INVALID", "{query:?} {limit}");
        assert!(!error.retryable());
    }
    let folder = fx.dir.0.join("dropped-folder");
    std::fs::create_dir_all(&folder).unwrap();
    let error = fx
        .service
        .import_files(place.id, "", vec![folder])
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_QUERY_INVALID");
}

#[tokio::test]
async fn malformed_cursor() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let not_a_key = Cursor::encode(&"just a string");
    let error = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, Some(not_a_key.clone()))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    assert!(!error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    let garbage: Cursor = serde_json::from_str("\"***not base64***\"").unwrap();
    let error = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, Some(garbage))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    let folder = work_folder(&fx, None, vec![]);
    let error = folder
        .operator_list("/", Some(Cursor::encode(&7_u32)))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
}

#[tokio::test]
async fn agent_folder_manager_refused_the_grants() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let workspace = WorkspaceId::new();
    let agent = ordin_types::AgentInstanceId::new();
    // A grant of another workspace in this workspace's set is refused by the Agent Folder Manager.
    let grant = FolderGrant {
        id: FolderGrantId::new(),
        workspace_id: WorkspaceId::new(),
        grantee: Grantee::AgentInstance {
            agent_instance_id: agent,
        },
        place_id: place.id,
        access: FolderAccess::Read,
        granted_at: Timestamp::now(),
    };
    let error = fx
        .service
        .publish_grants(workspace, vec![grant])
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_GRANT_HANDOFF_FAILED");
    assert!(error.retryable());
    // The Rules Engine was told all the same, so it is never more permissive than the stored grants.
    assert!(fx.rules.may_read(&ordin_rules::PlaceRead {
        workspace_id: workspace,
        reader: ordin_rules::PlaceReader::Agent {
            agent_instance_id: agent
        },
        place_id: place.id,
        path: "a.md".to_owned(),
    }));
    // A grant whose place is gone is dropped and logged, not an error.
    let gone = FolderGrant {
        id: FolderGrantId::new(),
        workspace_id: workspace,
        grantee: Grantee::AgentInstance {
            agent_instance_id: agent,
        },
        place_id: PlaceId::new(),
        access: FolderAccess::Read,
        granted_at: Timestamp::now(),
    };
    fx.service
        .publish_grants(workspace, vec![gone])
        .await
        .unwrap();
    assert!(!fx.rules.may_read(&ordin_rules::PlaceRead {
        workspace_id: workspace,
        reader: ordin_rules::PlaceReader::Agent {
            agent_instance_id: agent
        },
        place_id: place.id,
        path: "a.md".to_owned(),
    }));
}

#[tokio::test]
async fn watcher_cannot_be_started_on_a_place() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    std::fs::remove_dir_all(fx.root(&place)).unwrap();
    let error = fx.service.watch(&[place.id]).await.unwrap_err();
    assert_eq!(error.code(), "FILE_WATCH_FAILED");
    assert!(error.retryable());
    fx.service.shutdown().await;
}

#[tokio::test]
async fn agent_instance_has_no_grant_on_the_place() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let error = fx
        .service
        .read_for_agent(
            ordin_types::AgentInstanceId::new(),
            WorkspaceId::new(),
            place.id,
            "a.md",
            10,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_NOT_GRANTED");
    assert!(!error.retryable());
}

#[tokio::test]
async fn incoming_file_fails_the_safety_check() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let refused = |error: FileError| match error {
        FileError::IncomingRefused { reason } => reason,
        other => panic!("expected FILE_INCOMING_REFUSED, got {other:?}"),
    };
    // A name that cannot be used.
    for name in ["", ".hidden", "a\\b.txt"] {
        let error = fx
            .service
            .save_incoming(place.id, name, b"x".to_vec(), "n")
            .await
            .unwrap_err();
        assert_eq!(error.code(), "FILE_INCOMING_REFUSED");
        assert_eq!(refused(error), "The file's name can't be used.");
    }
    // An extension that runs as a program.
    let reason = refused(
        fx.service
            .save_incoming(place.id, "setup.EXE", b"x".to_vec(), "n")
            .await
            .unwrap_err(),
    );
    assert_eq!(
        reason,
        "This file can run as a program, so ordin didn't save it."
    );
    // Program headers under a harmless name.
    for header in [
        &b"MZ"[..],
        &[0x7F, b'E', b'L', b'F'],
        &[0xFE, 0xED, 0xFA, 0xCE],
        &[0xFE, 0xED, 0xFA, 0xCF],
        &[0xCE, 0xFA, 0xED, 0xFE],
        &[0xCF, 0xFA, 0xED, 0xFE],
        &[0xCA, 0xFE, 0xBA, 0xBE],
    ] {
        let error = fx
            .service
            .save_incoming(place.id, "x.pdf", header.to_vec(), "n")
            .await
            .unwrap_err();
        assert_eq!(error.code(), "FILE_INCOMING_REFUSED");
    }
    // An Office archive with macros.
    let docx = zip_with(&[
        ("word/document.xml", "<w:t>x</w:t>"),
        ("word/vbaProject.bin", "macro"),
    ]);
    let reason = refused(
        fx.service
            .save_incoming(place.id, "memo.docx", docx, "n")
            .await
            .unwrap_err(),
    );
    assert_eq!(
        reason,
        "This document holds macros, so ordin didn't save it."
    );
    // Not a workspace folder, and too large.
    let persons = fx.place("P", PlaceKind::PersonsFolder).await;
    assert_eq!(
        fx.service
            .save_incoming(persons.id, "a.txt", b"x".to_vec(), "n")
            .await
            .unwrap_err()
            .code(),
        "FILE_PROTECTED_LOCATION"
    );
    let big = vec![0u8; (crate::READ_LIMIT_BYTES + 1) as usize];
    assert_eq!(
        fx.service
            .save_incoming(place.id, "big.txt", big, "n")
            .await
            .unwrap_err()
            .code(),
        "FILE_TOO_LARGE"
    );
    assert_eq!(
        std::fs::read_dir(fx.root(&place)).unwrap().count(),
        0,
        "nothing was written"
    );
}

#[tokio::test]
async fn a_file_already_exists_where_a_binned_file_would_be_restored() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "a.md", b"old");
    let ticket = fx.service.move_to_bin(place.id, "a.md").await.unwrap();
    std::fs::write(&abs, b"new").unwrap();
    let error = fx.service.restore_from_bin(&ticket).await.unwrap_err();
    assert_eq!(error.code(), "FILE_RESTORE_TARGET_EXISTS");
    assert!(!error.retryable());
    assert_eq!(std::fs::read(&abs).unwrap(), b"new");
}

#[tokio::test]
async fn agent_file_tool_refused_by_the_work_folder() {
    let fx = fixture().await;
    let folder = work_folder(&fx, Some(vec!["/plans"]), vec![]);
    let refusal: AgentRefusal = folder.write("/AGENTS.md", "x").await.unwrap_err();
    // The refusal text goes to the model verbatim; the Agent Loader reports it as FILE_AGENT_REFUSED.
    assert_eq!(
        refusal.to_string(),
        "Error: /AGENTS.md is read-only (bundle path)"
    );
    assert!(ALL_CODES.contains(&"FILE_AGENT_REFUSED"));
    assert_eq!(
        lookup("FILE_AGENT_REFUSED").unwrap().owner_crate,
        "ordin-files"
    );
}

#[tokio::test]
async fn datastore_unreachable() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.store.close().await;
    let error = fx.service.place(place.id).await.unwrap_err();
    assert_eq!(error.code(), "FILE_DATASTORE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    assert_eq!(
        fx.service
            .list_folder(place.id, "", FolderSort::NameAsc, None)
            .await
            .unwrap_err()
            .code(),
        "FILE_DATASTORE_UNAVAILABLE"
    );
}

#[tokio::test]
async fn work_folder_invalid_path_operator() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    for path in ["no-slash", "/a/../b", "/a/~b", "/a\0b"] {
        let error = folder.operator_read(path).await.unwrap_err();
        assert_eq!(error.code(), "WF_INVALID_PATH", "{path:?}");
        assert!(!error.retryable());
    }
    assert_eq!(
        crate::normalise_path("x").unwrap_err().code(),
        "WF_INVALID_PATH"
    );
    // A writable path in the bundle is refused when the folder is built.
    let error = crate::WorkFolder::new(
        fx.store.clone(),
        crate::WorkFolderConfig {
            namespace: "ns".into(),
            bundle_dir: fx.dir.0.join("bundle"),
            writable_paths: Some(vec!["/knowledge/x".into()]),
            protected_paths: vec![],
        },
    )
    .err()
    .expect("refused");
    assert_eq!(error.code(), "WF_INVALID_PATH");
}

#[tokio::test]
async fn work_folder_bundle_path_to_an_operator_method() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    for path in ["/AGENTS.md", "/knowledge/note.md", "/.apm/x"] {
        assert_eq!(
            folder.operator_read(path).await.unwrap_err().code(),
            "WF_BUNDLE_PATH"
        );
        assert_eq!(
            folder
                .operator_write(path, vec![])
                .await
                .unwrap_err()
                .code(),
            "WF_BUNDLE_PATH"
        );
        assert_eq!(
            folder.operator_remove(path).await.unwrap_err().code(),
            "WF_BUNDLE_PATH"
        );
        assert_eq!(
            folder.operator_exists(path).await.unwrap_err().code(),
            "WF_BUNDLE_PATH"
        );
    }
    assert_eq!(
        folder
            .operator_list("/knowledge/", None)
            .await
            .unwrap_err()
            .code(),
        "WF_BUNDLE_PATH"
    );
}

#[tokio::test]
async fn work_folder_no_file_operator() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    let error = folder.operator_read("/plans/none.md").await.unwrap_err();
    assert_eq!(error.code(), "WF_NOT_FOUND");
    assert!(!error.retryable());
    assert_eq!(
        folder
            .operator_remove("/plans/none.md")
            .await
            .unwrap_err()
            .code(),
        "WF_NOT_FOUND"
    );
}

#[tokio::test]
async fn work_folder_above_10485760_bytes_operator() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    let at_limit = vec![b'a'; crate::WF_MAX_FILE_BYTES];
    folder.operator_write("/big.txt", at_limit).await.unwrap();
    let error = folder
        .operator_write("/big.txt", vec![b'a'; crate::WF_MAX_FILE_BYTES + 1])
        .await
        .unwrap_err();
    assert_eq!(error.code(), "WF_TOO_LARGE");
    assert!(!error.retryable());
    assert_eq!(crate::MAX_FILE_BYTES, crate::WF_MAX_FILE_BYTES);
}

#[tokio::test]
async fn work_folder_store_unreachable_operator() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    fx.store.close().await;
    let error = folder.operator_read("/plans/a.md").await.unwrap_err();
    assert_eq!(error.code(), "WF_STORE_UNAVAILABLE");
    assert!(error.retryable());
    for error in [
        folder.operator_write("/a.md", vec![1]).await.unwrap_err(),
        folder.operator_remove("/a.md").await.unwrap_err(),
        folder.operator_exists("/a.md").await.unwrap_err(),
        folder.operator_list("/", None).await.unwrap_err(),
    ] {
        assert_eq!(error.code(), "WF_STORE_UNAVAILABLE");
    }
}

#[tokio::test]
async fn work_folder_namespace_does_not_match_its_pattern() {
    let fx = fixture().await;
    for namespace in ["", "has space", "slash/ed", &"n".repeat(65)] {
        let error = crate::WorkFolder::new(
            fx.store.clone(),
            crate::WorkFolderConfig {
                namespace: namespace.to_owned(),
                bundle_dir: fx.dir.0.clone(),
                writable_paths: None,
                protected_paths: vec![],
            },
        )
        .err()
        .expect("refused");
        assert_eq!(error.code(), "WF_INVALID_NAMESPACE", "{namespace:?}");
        assert!(!error.retryable());
    }
    // A bundle folder that is not a folder is FILE_FOLDER_UNREADABLE.
    let error = crate::WorkFolder::new(
        fx.store.clone(),
        crate::WorkFolderConfig {
            namespace: "ns".into(),
            bundle_dir: fx.dir.0.join("nothing-here"),
            writable_paths: None,
            protected_paths: vec![],
        },
    )
    .err()
    .expect("refused");
    assert_eq!(error.code(), "FILE_FOLDER_UNREADABLE");
}

#[tokio::test]
async fn work_folder_agent_write_refused_bundle_writable_protected_size_store() {
    let fx = fixture().await;
    let folder = work_folder(
        &fx,
        Some(vec!["/plans", "/.agent-run"]),
        vec!["/.agent-run/duty/STOP", "/.agent-run/duty/escalation.md"],
    );
    // Bundle, writable paths, size: the first failing test wins.
    assert_eq!(
        folder.write("/knowledge/x.md", "x").await.unwrap_err().0,
        "Error: /knowledge/x.md is read-only (bundle path)"
    );
    assert_eq!(
        folder.write("/notes/x.md", "x").await.unwrap_err().0,
        "Error: /notes/x.md is outside the writable paths of this agent"
    );
    let big = "a".repeat(crate::WF_MAX_FILE_BYTES + 1);
    assert_eq!(
        folder.write("/plans/big.md", &big).await.unwrap_err().0,
        "Error: /plans/big.md exceeds the limit of 10485760 bytes"
    );
    // Protected: STOP and escalation.md can be written, not removed.
    folder.write("/.agent-run/duty/STOP", "stop").await.unwrap();
    folder
        .write("/.agent-run/duty/escalation.md", "help")
        .await
        .unwrap();
    let held = folder.delete("/.agent-run/duty/STOP").await.unwrap_err().0;
    assert_eq!(
        held,
        "Error: /.agent-run/duty/STOP cannot be removed by the agent; a person removes /.agent-run/duty/STOP with the operator's remove command"
    );
    assert_eq!(
        folder.delete("/").await.unwrap_err().0,
        "Error: / cannot be deleted"
    );
    assert_eq!(
        folder.delete("/AGENTS.md").await.unwrap_err().0,
        "Error: /AGENTS.md is read-only (bundle path)"
    );
    // Store unreachable.
    fx.store.close().await;
    assert_eq!(
        folder.write("/plans/a.md", "x").await.unwrap_err().0,
        "Error: the work folder cannot be reached"
    );
}

fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        for (name, body) in files {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }
    out
}

#[tokio::test]
async fn move_file_onto_a_path_where_another_file_exists_and_outside_a_workspace_folder() {
    // The two rows of the ADR-104 amendment; the Gherkin tests in `moving.rs` cover the same cases.
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    fx.put(&place, "a.md", b"a");
    fx.put(&place, "b.md", b"b");
    let error = fx
        .service
        .move_file(place.id, "a.md", "b.md")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_MOVE_TARGET_EXISTS");
    assert!(!error.retryable());
    let persons = fx.place("P", PlaceKind::PersonsFolder).await;
    assert_eq!(
        fx.service
            .move_file(persons.id, "a.md", "b.md")
            .await
            .unwrap_err()
            .code(),
        "FILE_PROTECTED_LOCATION"
    );
}

#[test]
fn x4_every_code_is_registered_to_this_crate() {
    assert!(!ALL_CODES.is_empty());
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-files", "{code}");
        let prefix = code.split('_').next().unwrap();
        assert!(owns_prefix("ordin-files", prefix), "{prefix}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, ALL_CODES,
        "ALL_CODES is sorted and has no duplicates"
    );
    let id = PlaceId::new();
    let raised = [
        FileError::PlaceNotFound,
        FileError::NotFound,
        FileError::ProtectedLocation,
        FileError::FolderUnreadable,
        FileError::PlaceExists { existing: id },
        FileError::PathEscapesPlace,
        FileError::TooLarge { limit_bytes: 1 },
        FileError::ChangedMeanwhile {
            current: crate::Sha256Hex("a".repeat(64)),
        },
        FileError::WriteFailed,
        FileError::BinUnavailable,
        FileError::VersionNotFound,
        FileError::VersionCopyMissing,
        FileError::CopyNotFound,
        FileError::CopyTransitionInvalid {
            from: PublishedCopyState::Current,
            to: PublishedCopyState::Removed,
        },
        FileError::QueryInvalid,
        FileError::GrantHandoffFailed,
        FileError::WatchFailed,
        FileError::DatastoreUnavailable,
        FileError::NotGranted,
        FileError::IncomingRefused {
            reason: String::new(),
        },
        FileError::RestoreTargetExists,
        FileError::MoveTargetExists,
        FileError::Invalid("FVER_VERSION_GAP"),
        FileError::WorkFolder("WF_NOT_FOUND"),
    ];
    for error in raised {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
        assert_eq!(error.to_string(), error.code(), "Display is the code");
        assert_eq!(CodedError::code(&error), error.code());
        assert_eq!(CodedError::retryable(&error), error.retryable());
        // The registry agrees about retries.
        let entry = lookup(error.code()).unwrap();
        assert_eq!(entry.retryable, error.retryable(), "{}", error.code());
    }
    // The 2C and work-folder codes carried as strings are registered too.
    for code in [
        "PLACE_NAME_INVALID",
        "PLACE_PATH_INVALID",
        "FVER_PATH_INVALID",
        "FVER_LATEST_PRUNE",
        "PUB_FORMAT_MAKER_MISMATCH",
        "WF_STORE_UNAVAILABLE",
        "WF_INVALID_NAMESPACE",
    ] {
        assert!(ALL_CODES.contains(&code), "{code}");
    }
    assert_eq!(lookup("SRV_BAD_CURSOR").unwrap().owner_crate, "ordin-types");
    assert_eq!(
        FileError::from(ordin_types::TypesError::BadCursor).code(),
        "SRV_BAD_CURSOR"
    );
}
