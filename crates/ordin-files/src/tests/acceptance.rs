//! X2: one test per Gherkin scenario of FileService.md, named after it. Scenarios that name
//! 100,000 files run at a smaller size (see the notes on the two search scenarios).

use std::time::{Duration, Instant};

use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, FolderGrantId, PublishedCopyId, RunId, WorkspaceId};

use super::support::*;
use crate::types::{AddPlace, FolderSort, PlaceChangeKind, WriteVersion};
use crate::{
    CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileError, FileVersionOrigin,
    FolderAccess, FolderGrant, Grantee, PlaceKind, PublishedCopy, PublishedCopyState, PublishedVia,
};

#[tokio::test]
async fn happy_path_add_a_place_and_list_its_first_page() {
    let fx = fixture().await;
    let folder = fx.dir.0.join("places").join("Supplier quotes");
    std::fs::create_dir_all(&folder).unwrap();
    for n in 0..250 {
        std::fs::write(folder.join(format!("f{n:03}.txt")), b"x").unwrap();
    }
    let place = fx
        .service
        .add_place(AddPlace {
            path: folder,
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap();
    assert_eq!(place.friendly_name, "Supplier quotes");
    assert_eq!(fx.entries("place_added").await.len(), 1);
    let first = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, None)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 100);
    assert!(first.next_cursor.is_some());
    assert_eq!(first.items[0].name, "f000.txt");
    let second = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, first.next_cursor)
        .await
        .unwrap();
    assert_eq!(second.items.len(), 100);
    assert_eq!(second.items[0].name, "f100.txt");
    let third = fx
        .service
        .list_folder(place.id, "", FolderSort::NameAsc, second.next_cursor)
        .await
        .unwrap();
    assert_eq!(third.items.len(), 50);
    assert!(third.next_cursor.is_none());
    assert_eq!(third.items[49].name, "f249.txt");
}

#[tokio::test]
async fn error_path_a_protected_system_location_is_refused() {
    let fx = fixture().await;
    let error = fx
        .service
        .add_place(AddPlace {
            path: fx.protected.clone(),
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PROTECTED_LOCATION");
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM places")
        .fetch_one(fx.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0, "no place row exists");
}

// Unix only: a folder is made unreadable with mode bits, which Windows does not have.
#[cfg(unix)]
#[tokio::test]
async fn error_path_an_unreadable_folder_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let fx = fixture().await;
    let folder = fx.dir.0.join("places").join("closed");
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
}

// Unix only: a symbolic link needs a privilege on Windows.
#[cfg(unix)]
#[tokio::test]
async fn edge_case_the_same_folder_added_twice_is_one_place() {
    let fx = fixture().await;
    let place = fx.place("Supplier quotes", PlaceKind::PersonsFolder).await;
    let link = fx.dir.0.join("link");
    std::os::unix::fs::symlink(fx.root(&place), &link).unwrap();
    let error = fx
        .service
        .add_place(AddPlace {
            path: link,
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap_err();
    match error {
        FileError::PlaceExists { existing } => assert_eq!(existing, place.id),
        other => panic!("expected FILE_PLACE_EXISTS, got {other:?}"),
    }
}

// FS-R1-1: a person's edit is a new version by the person, announced as not made by Ordin.
// spec: ordin-desktop/components/FileService#FS-R1-1
#[tokio::test]
async fn happy_path_a_person_s_edit_is_a_new_version_by_the_person() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "Q3/notes.md", b"first");
    let mut changes = fx.service.subscribe_changes();
    let version = fx
        .service
        .write_person_edit(place.id, "Q3/notes.md", b"second".to_vec())
        .await
        .unwrap();
    assert_eq!(version.origin, FileVersionOrigin::Person);
    assert_eq!(std::fs::read(&abs).unwrap(), b"second");
    let change = next_change(&mut changes).await;
    assert_eq!(change.change, PlaceChangeKind::Modified);
    assert!(!change.by_ordin);
}

// FS-R1-1: the refusals are write_version's.
#[tokio::test]
async fn error_path_a_person_s_edit_that_leaves_the_place_is_refused() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let refused = fx
        .service
        .write_person_edit(place.id, "../outside.md", b"x".to_vec())
        .await
        .unwrap_err();
    let by_write_version = fx
        .service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "../outside.md".into(),
            bytes: b"x".to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap_err();
    assert_eq!(refused.code(), by_write_version.code());
}

#[tokio::test]
async fn happy_path_a_run_s_write_keeps_the_earlier_content_as_version_1() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "Q3/summary.md", b"the person's text");
    let mut changes = fx.service.subscribe_changes();
    let run_id = RunId::new();
    fx.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "Q3/summary.md".into(),
            bytes: b"# Q3 summary\n".to_vec(),
            origin: FileVersionOrigin::Run {
                run_id,
                tool: "write_file".into(),
            },
            expected_hash: None,
        })
        .await
        .unwrap();
    let versions = fx
        .service
        .versions(place.id, "Q3/summary.md")
        .await
        .unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(
        (versions[0].version, &versions[0].origin),
        (1, &FileVersionOrigin::Person)
    );
    assert_eq!(versions[1].version, 2);
    assert!(
        matches!(&versions[1].origin, FileVersionOrigin::Run { run_id: r, tool } if *r == run_id && tool == "write_file")
    );
    assert_eq!(std::fs::read(&abs).unwrap(), b"# Q3 summary\n");
    let change = next_change(&mut changes).await;
    assert_eq!(change.change, PlaceChangeKind::Modified);
    assert!(change.by_ordin);
    let earlier = fx.service.read_version(versions[0].id).await.unwrap();
    assert_eq!(earlier.bytes, b"the person's text");
}

#[tokio::test]
async fn error_path_a_write_after_the_file_changed_is_refused() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "Q3/summary.md", b"first");
    let read = fx
        .service
        .read_file(place.id, "Q3/summary.md")
        .await
        .unwrap();
    std::fs::write(&abs, b"the person edited it").unwrap();
    let error = fx
        .service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "Q3/summary.md".into(),
            bytes: b"agent text".to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: Some(read.hash),
        })
        .await
        .unwrap_err();
    match &error {
        FileError::ChangedMeanwhile { current } => {
            assert_eq!(current.0, crate::versions::hash_of(b"the person edited it"));
        }
        other => panic!("expected FILE_CHANGED_MEANWHILE, got {other:?}"),
    }
    assert_eq!(std::fs::read(&abs).unwrap(), b"the person edited it");
}

#[tokio::test]
async fn error_path_a_path_that_escapes_the_place_is_refused() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let error = fx
        .service
        .read_file(place.id, "../secrets.txt")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_PATH_ESCAPES_PLACE");
}

#[tokio::test]
async fn edge_case_a_change_made_while_ordin_was_closed_is_found_at_start() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "Q3/rates.md", b"first rates");
    fx.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "Q3/rates.md".into(),
            bytes: b"second rates".to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap();
    fx.service.watch(&[place.id]).await.unwrap();
    wait_ready(&fx, &place).await;
    fx.service.shutdown().await;
    // Edited in another app while ordin was not running.
    std::fs::write(&abs, b"edited in another app, so a longer text").unwrap();

    let restarted = second_service(&fx).await;
    let mut changes = restarted.subscribe_changes();
    restarted.start().await.unwrap();
    let change = next_change(&mut changes).await;
    assert_eq!(change.path, "Q3/rates.md");
    assert_eq!(change.change, PlaceChangeKind::Modified);
    assert!(!change.by_ordin);
    assert_eq!(change.version, Some(3));
    assert_eq!(change.origin, Some(FileVersionOrigin::Person));
    let versions = restarted.versions(place.id, "Q3/rates.md").await.unwrap();
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[2].origin, FileVersionOrigin::Person);
    restarted.shutdown().await;
}

/// The scenario names 100,000 files across 20 workspaces; this run indexes 2,000 files across 4
/// places (the 100,000-file figure is a performance target, not a test input).
#[tokio::test]
async fn happy_path_search_finds_a_name_and_a_text_match_within_one_second() {
    let fx = fixture().await;
    let mut ids = Vec::new();
    for n in 0..4 {
        let place = fx.place(&format!("W{n}"), PlaceKind::WorkspaceFolder).await;
        for f in 0..500 {
            fx.put(
                &place,
                &format!("docs/doc-{f}.md"),
                format!("filler text {f}").as_bytes(),
            );
        }
        if n == 2 {
            fx.put(&place, "Kestrel day rate.md", b"short");
            fx.put(
                &place,
                "Kestrel quote.md",
                "Kestrel quote: senior consultants, a day rate of £640 for the first month, then a lower day rate"
                    .repeat(4)
                    .as_bytes(),
            );
        }
        fx.service.rescan_place(place.id).await.unwrap();
        ids.push(place.id);
    }
    let started = Instant::now();
    let results = fx
        .service
        .search(&ids, "Kestrel day rate", 20)
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(results.hits.len(), 2, "{:?}", results.hits);
    assert!(results.hits[0].name_match, "name matches come first");
    assert_eq!(results.hits[0].path, "Kestrel day rate.md");
    assert!(!results.hits[1].name_match);
    assert_eq!(results.hits[1].path, "Kestrel quote.md");
    let snippet = results.hits[1].snippet.as_deref().unwrap();
    assert!(snippet.chars().count() <= 160 && snippet.contains("day rate"));
    assert_eq!(results.coverage.indexed, 2002);
}

/// The scenario's 41,200 of 58,000 are set on the place row the way a build in progress leaves them.
#[tokio::test]
async fn edge_case_search_while_the_index_is_still_being_built_reports_coverage() {
    let fx = fixture().await;
    let place = fx.place("Big", PlaceKind::PersonsFolder).await;
    sqlx::query(
        "UPDATE places SET index_state = 'building', index_indexed = 41200, index_total = 58000 \
         WHERE id = ?1",
    )
    .bind(place.id.to_string())
    .execute(fx.pool())
    .await
    .unwrap();
    let results = fx.service.search(&[place.id], "plan", 10).await.unwrap();
    assert_eq!(results.coverage.indexed, 41_200);
    assert_eq!(results.coverage.total, 58_000);
    assert_eq!(
        fx.service.coverage(&[place.id]).await.unwrap(),
        results.coverage
    );
}

#[tokio::test]
async fn error_path_the_system_bin_is_unavailable() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "a.md", b"keep me");
    *fx.bin.available.lock().unwrap() = false;
    let error = fx.service.move_to_bin(place.id, "a.md").await.unwrap_err();
    assert_eq!(error.code(), "FILE_BIN_UNAVAILABLE");
    assert!(abs.exists(), "the file still exists");
}

#[tokio::test]
async fn happy_path_grants_reach_the_agent_folder_manager_and_the_rules_engine() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let workspace = WorkspaceId::new();
    let agent = AgentInstanceId::new();
    let grant = FolderGrant {
        id: FolderGrantId::new(),
        workspace_id: workspace,
        grantee: Grantee::AgentInstance {
            agent_instance_id: agent,
        },
        place_id: place.id,
        access: FolderAccess::ReadAndChange,
        granted_at: Timestamp::now(),
    };
    fx.service
        .publish_grants(workspace, vec![grant])
        .await
        .unwrap();
    let held: Vec<(String, String)> = sqlx::query_as(
        "SELECT place_id, path FROM workspace_folder_grants WHERE workspace_id = ?1",
    )
    .bind(workspace.to_string())
    .fetch_all(fx.pool())
    .await
    .unwrap();
    assert_eq!(held, vec![(place.id.to_string(), place.path.clone())]);
    let may = fx.rules.may_read(&ordin_rules::PlaceRead {
        workspace_id: workspace,
        reader: ordin_rules::PlaceReader::Agent {
            agent_instance_id: agent,
        },
        place_id: place.id,
        path: "a.md".to_owned(),
    });
    assert!(may, "the Rules Engine holds a GrantedPlace for the place");
    assert_eq!(fx.entries("grants_published").await.len(), 1);
}

fn copy_in(state: PublishedCopyState) -> PublishedCopy {
    PublishedCopy {
        id: PublishedCopyId::new(),
        original: CopyOriginal::PlaceFile {
            place_id: crate::PlaceId::new(),
            path: "a.md".to_owned(),
        },
        destination: CopyDestination::ConfluencePage {
            space: "ENG".to_owned(),
            parent_page_id: "123".to_owned(),
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
        state,
        failure_reason: None,
    }
}

#[tokio::test]
async fn error_path_a_published_copy_cannot_skip_a_state() {
    let fx = fixture().await;
    let copy = fx
        .service
        .record_published_copy(copy_in(PublishedCopyState::ReadyToPublish))
        .await
        .unwrap();
    let error = fx
        .service
        .set_published_copy_state(copy.id, PublishedCopyState::Current, None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        FileError::CopyTransitionInvalid {
            from: PublishedCopyState::ReadyToPublish,
            to: PublishedCopyState::Current
        }
    ));
    assert_eq!(error.code(), "FILE_COPY_TRANSITION_INVALID");
}

#[tokio::test]
async fn edge_case_a_person_s_saved_edit_to_a_working_copy_document_is_held() {
    let fx = fixture().await;
    let place = fx.place("Copy", PlaceKind::WorkingCopy).await;
    fx.put(&place, "Supplier summary.md", b"# Summary");
    let version = fx
        .service
        .save_person_edit(
            place.id,
            "Supplier summary.md",
            b"# Summary, edited".to_vec(),
        )
        .await
        .unwrap();
    let held = fx.service.held_edits(&[place.id]).await.unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].path, "Supplier summary.md");
    assert_eq!(held[0].file_version_id, version.id);
    fx.service
        .mark_edit_sent(place.id, "Supplier summary.md")
        .await
        .unwrap();
    assert!(fx.service.held_edits(&[place.id]).await.unwrap().is_empty());
}

#[tokio::test]
async fn error_path_the_agent_cannot_remove_a_duty_s_stop_file() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec!["/.agent-run/watch-readiness/STOP"]);
    folder
        .operator_write("/.agent-run/watch-readiness/STOP", b"stop".to_vec())
        .await
        .unwrap();
    let refusal = folder
        .delete("/.agent-run/watch-readiness")
        .await
        .unwrap_err();
    assert!(
        refusal.0.contains("cannot be removed by the agent"),
        "{}",
        refusal.0
    );
    assert!(folder
        .operator_exists("/.agent-run/watch-readiness/STOP")
        .await
        .unwrap());
    folder
        .operator_remove("/.agent-run/watch-readiness")
        .await
        .unwrap();
    assert!(!folder
        .operator_exists("/.agent-run/watch-readiness/STOP")
        .await
        .unwrap());
}

#[tokio::test]
async fn error_path_a_bundle_path_is_read_only_for_the_agent() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    let refusal = folder.write("/AGENTS.md", "changed").await.unwrap_err();
    assert_eq!(refusal.0, "Error: /AGENTS.md is read-only (bundle path)");
}

#[tokio::test]
async fn edge_case_an_empty_writable_paths_list_makes_the_work_folder_read_only_for_the_agent() {
    let fx = fixture().await;
    let folder = work_folder(&fx, Some(vec![]), vec![]);
    let refusal = folder.write("/plans/a.md", "plan").await.unwrap_err();
    assert_eq!(
        refusal.0,
        "Error: /plans/a.md is outside the writable paths of this agent"
    );
}

#[tokio::test]
async fn happy_path_an_email_attachment_is_saved_after_the_safety_check() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    fx.put(&place, "quote.pdf", b"%PDF-1.4 the first quote");
    let mut pdf = b"%PDF-1.4\n".to_vec();
    pdf.extend(std::iter::repeat_n(b'a', 180_000));
    let saved = fx
        .service
        .save_incoming(place.id, "quote.pdf", pdf.clone(), "Mail from Kestrel")
        .await
        .unwrap();
    assert_eq!(saved.path, "quote (2).pdf");
    assert!(!saved.hidden_content_found);
    assert_eq!(
        std::fs::read(fx.root(&place).join("quote (2).pdf")).unwrap(),
        pdf
    );
    let received = fx.entries("file_received").await;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].data["place_id"], place.id.to_string());
    assert_eq!(received[0].data["origin_note"], "Mail from Kestrel");
}

#[tokio::test]
async fn error_path_a_program_received_by_email_is_refused() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let error = fx
        .service
        .save_incoming(
            place.id,
            "invoice.pdf",
            b"MZ\x90\x00 program".to_vec(),
            "Mail",
        )
        .await
        .unwrap_err();
    match &error {
        FileError::IncomingRefused { reason } => {
            assert_eq!(
                reason,
                "This file can run as a program, so ordin didn't save it."
            );
        }
        other => panic!("expected FILE_INCOMING_REFUSED, got {other:?}"),
    }
    assert!(
        !fx.root(&place).join("invoice.pdf").exists(),
        "no file is written"
    );
}

#[tokio::test]
async fn edge_case_hidden_text_is_reported_but_the_file_is_kept() {
    let fx = fixture().await;
    let place = fx.place("W", PlaceKind::WorkspaceFolder).await;
    let saved = fx
        .service
        .save_incoming(
            place.id,
            "notes.md",
            "# Notes\u{200B}\nhidden".as_bytes().to_vec(),
            "Telegram",
        )
        .await
        .unwrap();
    assert!(saved.hidden_content_found);
    assert!(fx.root(&place).join("notes.md").exists());
}

#[tokio::test]
async fn error_path_an_agent_without_a_grant_cannot_read_a_place() {
    let fx = fixture().await;
    let p = fx.place("P", PlaceKind::PersonsFolder).await;
    let q = fx.place("Q", PlaceKind::PersonsFolder).await;
    fx.put(&p, "a.txt", b"readable");
    fx.put(&q, "a.txt", b"not granted");
    let workspace = WorkspaceId::new();
    let agent = AgentInstanceId::new();
    // Before any grants are published, every read is refused.
    let early = fx
        .service
        .read_for_agent(agent, workspace, p.id, "a.txt", 100)
        .await;
    assert_eq!(early.unwrap_err().code(), "FILE_NOT_GRANTED");
    fx.service
        .publish_grants(
            workspace,
            vec![FolderGrant {
                id: FolderGrantId::new(),
                workspace_id: workspace,
                grantee: Grantee::AgentInstance {
                    agent_instance_id: agent,
                },
                place_id: p.id,
                access: FolderAccess::Read,
                granted_at: Timestamp::now(),
            }],
        )
        .await
        .unwrap();
    let error = fx
        .service
        .read_for_agent(agent, workspace, q.id, "a.txt", 100)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "FILE_NOT_GRANTED");
    let bytes = fx
        .service
        .read_for_agent(agent, workspace, p.id, "a.txt", 100)
        .await
        .unwrap();
    assert_eq!(bytes, b"readable");
}

#[tokio::test]
async fn happy_path_a_binned_file_is_put_back_from_its_ticket() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "notes/plan.md", b"the plan");
    let ticket = fx
        .service
        .move_to_bin(place.id, "notes/plan.md")
        .await
        .unwrap();
    assert!(!abs.exists());
    fx.service.restore_from_bin(&ticket).await.unwrap();
    assert_eq!(std::fs::read(&abs).unwrap(), b"the plan");
    let versions = fx
        .service
        .versions(place.id, "notes/plan.md")
        .await
        .unwrap();
    assert!(versions
        .iter()
        .any(|v| v.origin == FileVersionOrigin::Person));
    assert_eq!(fx.entries("file_binned").await.len(), 1);
    assert_eq!(fx.entries("file_restored").await.len(), 1);
}

#[tokio::test]
async fn error_path_restore_does_not_overwrite_a_new_file() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    let abs = fx.put(&place, "notes/plan.md", b"the plan");
    let ticket = fx
        .service
        .move_to_bin(place.id, "notes/plan.md")
        .await
        .unwrap();
    std::fs::write(&abs, b"a new file").unwrap();
    let error = fx.service.restore_from_bin(&ticket).await.unwrap_err();
    assert_eq!(error.code(), "FILE_RESTORE_TARGET_EXISTS");
    assert_eq!(std::fs::read(&abs).unwrap(), b"a new file");
}

#[tokio::test]
async fn happy_path_a_run_s_write_tells_subscribers_its_version() {
    let fx = fixture().await;
    let place = fx.place("P", PlaceKind::PersonsFolder).await;
    fx.put(&place, "Q3/summary.md", b"before");
    let mut changes = fx.service.subscribe_changes();
    let run_id = RunId::new();
    let origin = FileVersionOrigin::Run {
        run_id,
        tool: "write_file".into(),
    };
    fx.service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "Q3/summary.md".into(),
            bytes: b"after".to_vec(),
            origin: origin.clone(),
            expected_hash: None,
        })
        .await
        .unwrap();
    let change = next_change(&mut changes).await;
    assert_eq!(change.version, Some(2));
    assert_eq!(change.origin, Some(origin));
}

#[tokio::test]
async fn happy_path_the_operator_checks_a_work_folder_file_exists() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    folder
        .operator_write("/plans/a.md", b"plan".to_vec())
        .await
        .unwrap();
    assert!(folder.operator_exists("/plans/a.md").await.unwrap());
    assert!(!folder.operator_exists("/plans/b.md").await.unwrap());
    let error = folder.operator_exists("/AGENTS.md").await.unwrap_err();
    assert_eq!(error.code(), "WF_BUNDLE_PATH");
}

// FileService.md, Amendment January 2026 — Folder paths in their plain form on Windows. The
// operating system resolves a folder to `\\?\C:\...`; the place keeps the plain `C:\...` form.

// spec: ordin-desktop/components/FileService#FS-PATH-1
#[cfg(windows)]
#[tokio::test]
async fn happy_path_a_place_root_is_stored_in_the_plain_form() {
    let fx = fixture().await;
    // The fixture's folder is already in its plain form; the project folder below it is too.
    let plain = fx.dir.0.join("places").join("project");
    std::fs::create_dir_all(&plain).unwrap();
    let verbatim = std::fs::canonicalize(&plain).unwrap();
    assert!(
        verbatim.to_str().unwrap().starts_with(r"\\?\"),
        "{verbatim:?}"
    );
    let place = fx
        .service
        .add_place(AddPlace {
            path: verbatim,
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap();
    assert!(!place.path.starts_with(r"\\?\"), "{}", place.path);
    assert_eq!(place.path, plain.to_str().unwrap());
}

// spec: ordin-desktop/components/FileService#FS-PATH-1
#[cfg(windows)]
#[tokio::test]
async fn edge_case_a_path_inside_the_place_is_located() {
    let fx = fixture().await;
    let plain = fx.dir.0.join("places").join("project");
    std::fs::create_dir_all(plain.join("src")).unwrap();
    std::fs::write(plain.join("src").join("main.rs"), b"fn main() {}").unwrap();
    let place = fx
        .service
        .add_place(AddPlace {
            path: plain.clone(),
            friendly_name: None,
            kind: PlaceKind::PersonsFolder,
        })
        .await
        .unwrap();
    let file = plain.join("src").join("main.rs");
    assert_eq!(
        fx.service.locate(&file).await.unwrap(),
        Some((place.id, "src/main.rs".to_owned()))
    );
    // The same file named in the verbatim form lies in the same place.
    let verbatim = std::fs::canonicalize(&file).unwrap();
    assert!(
        verbatim.to_str().unwrap().starts_with(r"\\?\"),
        "{verbatim:?}"
    );
    assert_eq!(
        fx.service.locate(&verbatim).await.unwrap(),
        Some((place.id, "src/main.rs".to_owned()))
    );
}
