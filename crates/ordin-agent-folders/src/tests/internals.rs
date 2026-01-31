//! Behaviour of the Internal Logic steps beyond the Error Table and the Gherkin scenarios.

use std::sync::Arc;
use std::time::Duration;

use ordin_journal::{ApplyMode, EntryFilter, MigrationRegistry, MIGRATIONS};
use ordin_types::domain::places::{FolderAccess, PlaceKind};
use ordin_types::{CrewCharterId, PlaceId, RunId, WorkspaceId};

use super::support::{
    fixture, fixture_waiting, grant_for, instance, journal_in, to_crew, to_instance, TempDir,
};
use crate::{AfmError, AgentFolderManager, AgentFolderState, LockMode, MODULE};

#[test]
fn migrations_are_one_consecutive_step_on_each_engine() {
    let migrations = crate::migrations();
    assert_eq!(migrations.module, MODULE);
    assert_eq!(MODULE, "agent_folders");
    for steps in [migrations.sqlite, migrations.postgres] {
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].0, 1);
        for table in [
            "agent_folders",
            "session_folder_access",
            "workspace_folder_grants",
        ] {
            assert!(
                steps[0].2.contains(&format!("CREATE TABLE {table}")),
                "{table}"
            );
        }
    }
    assert!(migrations.sqlite[0].2.contains("STRICT"));
    assert!(migrations.postgres[0].2.contains("TIMESTAMPTZ"));
}

#[tokio::test]
async fn migrations_register_and_apply_once() {
    let fx = fixture().await;
    let store = fx.journal.datastore();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(crate::migrations()).unwrap();
    let report = registry
        .apply(store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    assert!(report.applied.is_empty(), "already applied at start");
    assert_eq!(
        fx.count("SELECT count(*) FROM schema_migrations WHERE module = 'agent_folders'")
            .await,
        1
    );
}

#[tokio::test]
async fn start_creates_the_agents_folder_and_leaves_no_probe() {
    let fx = fixture().await;
    let agents = fx.root.join("agents");
    assert!(agents.is_dir());
    assert_eq!(std::fs::read_dir(&agents).unwrap().count(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&agents).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}

#[tokio::test]
async fn start_creates_a_missing_data_root() {
    let dir = TempDir::new();
    let journal = journal_in(&dir).await;
    let root = dir.path().join("not").join("yet");
    AgentFolderManager::new(journal, root.clone())
        .await
        .unwrap();
    assert!(root.join("agents").is_dir());
}

#[tokio::test]
async fn create_get_round_trip_and_replace_of_a_removed_row() {
    let fx = fixture().await;
    let id = instance(1);
    let made = fx.afm.create(id).await.unwrap();
    assert_eq!(fx.afm.get(id).await.unwrap(), made);
    fx.afm.remove(id).await.unwrap();
    // An instance id is never reused by the Package Catalogue; a removed row is replaced.
    let again = fx.afm.create(id).await.unwrap();
    assert_eq!(again.state, AgentFolderState::Created);
    assert!(again.folder_path.is_dir());
    assert_eq!(fx.count("SELECT count(*) FROM agent_folders").await, 1);
}

#[tokio::test]
async fn create_accepts_an_empty_existing_folder() {
    let fx = fixture().await;
    let id = instance(2);
    let folder = fx.root.join("agents").join(id.to_string()).join("folder");
    std::fs::create_dir_all(&folder).unwrap();
    fx.afm.create(id).await.unwrap();
    assert!(folder.is_dir());
}

#[tokio::test]
async fn many_sessions_share_a_folder_and_a_second_session_update_is_refused() {
    let fx = fixture_waiting(Duration::from_millis(150)).await;
    let id = instance(3);
    fx.afm.create(id).await.unwrap();
    let _one = fx.afm.lock(id, LockMode::Session).await.unwrap();
    let _two = fx.afm.lock(id, LockMode::Session).await.unwrap();
    let _update = fx.afm.lock(id, LockMode::SessionUpdate).await.unwrap();
    let error = fx.afm.lock(id, LockMode::SessionUpdate).await.unwrap_err();
    assert_eq!(
        error,
        AfmError::FolderBusy {
            id,
            held: "session_update"
        }
    );
    let error = fx.afm.lock(id, LockMode::Write).await.unwrap_err();
    assert!(matches!(error, AfmError::FolderBusy { .. }));
}

#[tokio::test]
async fn a_waiting_write_is_granted_when_the_session_ends() {
    let fx = fixture().await;
    let id = instance(4);
    fx.afm.create(id).await.unwrap();
    let afm = Arc::new(fx.afm);
    let session = afm.lock(id, LockMode::Session).await.unwrap();
    let waiting = Arc::clone(&afm);
    let write =
        tokio::spawn(async move { waiting.lock(id, LockMode::Write).await.map(|l| l.mode()) });
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(session);
    assert_eq!(write.await.unwrap().unwrap(), LockMode::Write);
}

#[tokio::test]
async fn a_session_never_queues_behind_a_write() {
    let fx = fixture().await;
    let id = instance(5);
    fx.afm.create(id).await.unwrap();
    let _write = fx.afm.lock(id, LockMode::Write).await.unwrap();
    let started = std::time::Instant::now();
    let error = fx.afm.lock(id, LockMode::Session).await.unwrap_err();
    assert!(matches!(error, AfmError::FolderBusy { held: "write", .. }));
    assert!(started.elapsed() < Duration::from_secs(2), "no wait");
}

#[tokio::test]
async fn remove_is_refused_while_a_lease_is_held() {
    let fx = fixture_waiting(Duration::from_millis(150)).await;
    let id = instance(6);
    let folder = fx.afm.create(id).await.unwrap();
    let lease = fx.afm.lock(id, LockMode::Session).await.unwrap();
    let error = fx.afm.remove(id).await.unwrap_err();
    assert_eq!(
        error,
        AfmError::FolderBusy {
            id,
            held: "session"
        }
    );
    assert!(folder.folder_path.is_dir());
    drop(lease);
    fx.afm.remove(id).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn remove_deletes_a_link_and_leaves_its_target() {
    let fx = fixture().await;
    let id = instance(7);
    let folder = fx.afm.create(id).await.unwrap();
    let target = fx.dir.dir("target");
    std::fs::write(target.join("keep.txt"), b"x").unwrap();
    std::os::unix::fs::symlink(&target, folder.folder_path.join("link")).unwrap();
    fx.afm.remove(id).await.unwrap();
    assert!(target.join("keep.txt").exists());
    assert!(!folder.folder_path.exists());
}

#[tokio::test]
async fn remove_ends_the_sessions_of_the_instance() {
    let fx = fixture().await;
    let (a, w, p) = (instance(8), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let place = fx.dir.dir("place");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    let run = RunId::new();
    fx.afm.apply_grants(run, a, w, None).await.unwrap();
    assert_eq!(
        fx.count("SELECT count(*) FROM session_folder_access").await,
        1
    );
    fx.afm.remove(a).await.unwrap();
    assert_eq!(
        fx.count("SELECT count(*) FROM session_folder_access").await,
        0
    );
    assert_eq!(
        fx.afm.session_access(run).await.unwrap_err(),
        AfmError::SessionUnknown { run_id: run }
    );
}

#[tokio::test]
async fn recover_clears_sessions_and_counts_unknown_directories() {
    let fx = fixture().await;
    let (a, w, p) = (instance(9), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let place = fx.dir.dir("place");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    let run = RunId::new();
    fx.afm.apply_grants(run, a, w, None).await.unwrap();
    let stranger = fx.root.join("agents").join("not-recorded");
    std::fs::create_dir(&stranger).unwrap();
    std::fs::write(stranger.join("mine.txt"), b"x").unwrap();
    std::fs::write(fx.root.join("agents").join("a-plain-file"), b"x").unwrap();
    let entries_before = fx.kinds().await.len();

    let report = fx.afm.recover().await.unwrap();
    assert_eq!(report.sessions_cleared, 1);
    assert_eq!(report.unknown_directories, 1);
    assert_eq!(report.removals_finished, 0);
    assert!(
        stranger.join("mine.txt").exists(),
        "never deletes what ordin did not record"
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM session_folder_access").await,
        0
    );
    assert!(matches!(
        fx.afm.session_access(run).await,
        Err(AfmError::SessionUnknown { .. })
    ));
    assert_eq!(
        fx.kinds().await.len(),
        entries_before,
        "recover journals nothing"
    );
}

#[tokio::test]
async fn end_session_is_idempotent_and_deletes_the_rows() {
    let fx = fixture().await;
    let (a, w, p) = (instance(10), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let place = fx.dir.dir("place");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    let run = RunId::new();
    fx.afm.apply_grants(run, a, w, None).await.unwrap();
    fx.afm.end_session(run).await.unwrap();
    fx.afm.end_session(run).await.unwrap();
    fx.afm.end_session(RunId::new()).await.unwrap();
    assert!(matches!(
        fx.afm.session_access(run).await,
        Err(AfmError::SessionUnknown { .. })
    ));
}

#[tokio::test]
async fn a_session_with_no_grant_yet_receives_a_later_grant() {
    let fx = fixture().await;
    let (a, w, p) = (instance(11), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let run = RunId::new();
    let empty = fx.afm.apply_grants(run, a, w, None).await.unwrap();
    assert!(empty.grants.is_empty());
    assert!(fx.afm.session_access(run).await.unwrap().grants.is_empty());

    let place = fx.dir.dir("later");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    let access = fx.afm.session_access(run).await.unwrap();
    assert_eq!(access.grants.len(), 1);
    assert_eq!(access.grants[0].place_id, p);
    let entry = fx
        .journal
        .list_entries(
            EntryFilter {
                run_id: None,
                kind: Some("workspace_folder_grants_set".to_owned()),
            },
            None,
        )
        .await
        .unwrap()
        .items;
    assert_eq!(entry[0].data["sessions_updated"], 1);
    assert_eq!(entry[0].data["grants"], 1);
    assert_eq!(entry[0].data["skipped"], 0);
}

#[tokio::test]
async fn a_crew_grant_applies_only_with_its_charter_and_a_double_grant_keeps_the_broader() {
    let fx = fixture().await;
    let (a, w, c) = (instance(12), WorkspaceId::new(), CrewCharterId::new());
    let other_charter = CrewCharterId::new();
    fx.afm.create(a).await.unwrap();
    let (shared, only_crew) = (PlaceId::new(), PlaceId::new());
    let (shared_path, crew_path) = (fx.dir.dir("shared"), fx.dir.dir("crew"));
    fx.afm
        .set_workspace_grants(
            w,
            vec![
                grant_for(w, to_instance(a), shared, FolderAccess::Read, &shared_path),
                grant_for(
                    w,
                    to_crew(c),
                    shared,
                    FolderAccess::ReadAndChange,
                    &shared_path,
                ),
                grant_for(w, to_crew(c), only_crew, FolderAccess::Read, &crew_path),
            ],
        )
        .await
        .unwrap();
    let alone = fx.afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert_eq!(alone.grants.len(), 1);
    assert_eq!(alone.grants[0].access, FolderAccess::Read);
    let wrong = fx
        .afm
        .apply_grants(RunId::new(), a, w, Some(other_charter))
        .await
        .unwrap();
    assert_eq!(wrong.grants.len(), 1);
    let both = fx
        .afm
        .apply_grants(RunId::new(), a, w, Some(c))
        .await
        .unwrap();
    assert_eq!(both.grants.len(), 2);
    let broader = both.grants.iter().find(|g| g.place_id == shared).unwrap();
    assert_eq!(broader.access, FolderAccess::ReadAndChange);
}

#[tokio::test]
async fn resuming_a_run_repeats_the_derivation() {
    let fx = fixture().await;
    let (a, w, p) = (instance(13), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let place = fx.dir.dir("place");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    let run = RunId::new();
    let first = fx.afm.apply_grants(run, a, w, None).await.unwrap();
    let second = fx.afm.apply_grants(run, a, w, None).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(
        fx.count("SELECT count(*) FROM session_folder_access").await,
        1
    );
}

#[tokio::test]
async fn set_workspace_grants_leaves_out_unusable_grants_and_stores_the_rest() {
    let fx = fixture().await;
    let (a, w) = (instance(14), WorkspaceId::new());
    let good = fx.dir.dir("good");
    let (p_good, p_agent, p_relative, p_root) = (
        PlaceId::new(),
        PlaceId::new(),
        PlaceId::new(),
        PlaceId::new(),
    );
    let mut agent_kind = grant_for(w, to_instance(a), p_agent, FolderAccess::Read, &good);
    agent_kind.place_kind = PlaceKind::AgentFolder;
    fx.afm
        .set_workspace_grants(
            w,
            vec![
                grant_for(w, to_instance(a), p_good, FolderAccess::Read, &good),
                agent_kind,
                grant_for(
                    w,
                    to_instance(a),
                    p_relative,
                    FolderAccess::Read,
                    std::path::Path::new("relative/dir"),
                ),
                grant_for(w, to_instance(a), p_root, FolderAccess::Read, &fx.root),
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        fx.count("SELECT count(*) FROM workspace_folder_grants")
            .await,
        1
    );
    let entry = fx
        .journal
        .list_entries(
            EntryFilter {
                run_id: None,
                kind: Some("workspace_folder_grants_set".to_owned()),
            },
            None,
        )
        .await
        .unwrap()
        .items;
    assert_eq!(entry[0].data["grants"], 1);
    assert_eq!(entry[0].data["skipped"], 3);
}

#[tokio::test]
async fn journal_entries_carry_no_path() {
    let fx = fixture().await;
    let (a, w, p) = (instance(15), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let place = fx.dir.dir("private-place");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    fx.afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    fx.afm.remove(a).await.unwrap();
    let entries = fx
        .journal
        .list_entries(EntryFilter::default(), None)
        .await
        .unwrap()
        .items;
    let mine: Vec<_> = entries.iter().filter(|e| e.component == MODULE).collect();
    assert_eq!(
        mine.len(),
        4,
        "created, grants set, grants applied, removed"
    );
    let root = fx.root.to_str().unwrap();
    for entry in mine {
        let text = format!("{} {}", entry.summary, entry.data);
        assert!(
            !text.contains("private-place") && !text.contains(root),
            "{text}"
        );
    }
}

#[tokio::test]
async fn a_journal_failure_does_not_undo_the_operation() {
    let fx = fixture().await;
    fx.sql("DROP TABLE journal_entries").await;
    let id = instance(16);
    let folder = fx.afm.create(id).await.expect("the folder stands");
    assert!(folder.folder_path.is_dir());
    fx.afm.get(id).await.unwrap();
}

#[tokio::test]
async fn a_grant_on_an_unplugged_place_does_not_stop_the_session() {
    let fx = fixture().await;
    let (a, w) = (instance(17), WorkspaceId::new());
    fx.afm.create(a).await.unwrap();
    let (gone, here) = (PlaceId::new(), PlaceId::new());
    let here_path = fx.dir.dir("here");
    let gone_path = fx.dir.path().join("unplugged-drive");
    fx.afm
        .set_workspace_grants(
            w,
            vec![
                grant_for(w, to_instance(a), gone, FolderAccess::Read, &gone_path),
                grant_for(w, to_instance(a), here, FolderAccess::Read, &here_path),
            ],
        )
        .await
        .unwrap();
    let access = fx.afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert_eq!(access.grants.len(), 1);
    assert_eq!(access.grants[0].place_id, here);
}

#[cfg(unix)]
#[tokio::test]
async fn a_link_that_resolves_into_an_agent_folder_is_not_applied() {
    let fx = fixture().await;
    let (a, w, p, q) = (
        instance(18),
        WorkspaceId::new(),
        PlaceId::new(),
        PlaceId::new(),
    );
    let folder = fx.afm.create(a).await.unwrap();
    let link = fx.dir.path().join("shortcut");
    std::os::unix::fs::symlink(&folder.folder_path, &link).unwrap();
    let fine = fx.dir.dir("fine");
    fx.afm
        .set_workspace_grants(
            w,
            vec![
                grant_for(w, to_instance(a), p, FolderAccess::Read, &link),
                grant_for(w, to_instance(a), q, FolderAccess::Read, &fine),
            ],
        )
        .await
        .unwrap();
    let access = fx.afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert_eq!(access.grants.len(), 1);
    assert_eq!(access.grants[0].place_id, q);
}
