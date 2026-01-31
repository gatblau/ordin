//! X2: one test per Gherkin scenario of AgentFolderManager.md, named after the scenario. The two
//! scenarios that assert on a log line (`edge_a_granted_root_that_is_a_link_into_ordin_s_data_is_not_applied`
//! and `edge_grant_pointing_into_ordin_s_data_is_never_applied`) are in `tests/logging.rs`, alone in
//! their binary (plan FU-017).

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_types::domain::places::FolderAccess;
use ordin_types::{CrewCharterId, PlaceId, RunId, WorkspaceId};

use super::support::{
    fixture, fixture_waiting, grant_for, instance, to_crew, to_instance, TempDir,
};
use crate::{AfmError, AgentFolderManager, AgentFolderState, LockMode, WRITE_LOCK_WAIT};

#[tokio::test]
async fn happy_path_create_lock_for_a_session_remove() {
    let fx = fixture().await;
    let id = instance(1);
    let folder = fx.afm.create(id).await.unwrap();
    let instance_dir = fx.root.join("agents").join(id.to_string());
    assert_eq!(folder.folder_path, instance_dir.join("folder"));
    assert_eq!(folder.meta_path, instance_dir.join("meta"));
    assert_eq!(folder.state, AgentFolderState::Created);
    assert!(folder.folder_path.is_dir() && folder.meta_path.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in [&folder.folder_path, &folder.meta_path] {
            let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{dir:?}");
        }
    }
    assert!(fx
        .kinds()
        .await
        .contains(&"agent_folder_created".to_owned()));

    let lease = fx.afm.lock(id, LockMode::Session).await.unwrap();
    assert_eq!(lease.mode(), LockMode::Session);
    assert_eq!(lease.folder(), folder.folder_path);
    assert_eq!(lease.meta(), folder.meta_path);
    drop(lease);
    fx.afm.remove(id).await.unwrap();

    assert!(!folder.folder_path.exists() && !folder.meta_path.exists());
    assert!(!instance_dir.exists());
    assert_eq!(
        fx.count("SELECT count(*) FROM agent_folders WHERE state = 'removed'")
            .await,
        1
    );
    assert!(fx
        .kinds()
        .await
        .contains(&"agent_folder_removed".to_owned()));
}

#[tokio::test]
async fn error_session_access_for_an_unknown_run() {
    let fx = fixture().await;
    let run_id = RunId::new();
    let error = fx.afm.session_access(run_id).await.unwrap_err();
    assert_eq!(error, AfmError::SessionUnknown { run_id });
}

#[tokio::test]
async fn edge_a_pack_choice_update_during_a_session() {
    let fx = fixture().await;
    let id = instance(2);
    fx.afm.create(id).await.unwrap();
    let afm = Arc::new(fx.afm);
    let session = afm.lock(id, LockMode::Session).await.unwrap();
    let update = afm
        .lock(id, LockMode::SessionUpdate)
        .await
        .expect("granted beside a Session lease");
    assert_eq!(update.mode(), LockMode::SessionUpdate);

    let waiting = Arc::clone(&afm);
    let write =
        tokio::spawn(async move { waiting.lock(id, LockMode::Write).await.map(|l| l.mode()) });
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !write.is_finished(),
        "a Write lease requested meanwhile waits"
    );
    drop(update);
    drop(session);
    assert_eq!(write.await.unwrap().unwrap(), LockMode::Write);
}

#[tokio::test]
async fn edge_a_withdrawn_grant_reaches_a_live_session() {
    let fx = fixture().await;
    let (a, w, p) = (instance(3), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let place = fx.dir.dir("suppliers");
    fx.afm
        .set_workspace_grants(
            w,
            vec![grant_for(w, to_instance(a), p, FolderAccess::Read, &place)],
        )
        .await
        .unwrap();
    let run = RunId::new();
    let applied = fx.afm.apply_grants(run, a, w, None).await.unwrap();
    assert_eq!(applied.grants.len(), 1);
    assert_eq!(applied.grants[0].place_id, p);

    fx.afm.set_workspace_grants(w, vec![]).await.unwrap();
    let access = fx.afm.session_access(run).await.unwrap();
    assert!(access.grants.iter().all(|g| g.place_id != p));
    assert!(access.grants.is_empty());
}

#[tokio::test]
async fn happy_path_session_grants_are_derived_from_the_workspace_set() {
    let fx = fixture().await;
    let (a, w, c) = (instance(4), WorkspaceId::new(), CrewCharterId::new());
    let (p, q) = (PlaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    let (place_p, place_q) = (fx.dir.dir("p"), fx.dir.dir("q"));
    fx.afm
        .set_workspace_grants(
            w,
            vec![
                grant_for(w, to_instance(a), p, FolderAccess::Read, &place_p),
                grant_for(w, to_crew(c), q, FolderAccess::ReadAndChange, &place_q),
            ],
        )
        .await
        .unwrap();
    let access = fx
        .afm
        .apply_grants(RunId::new(), a, w, Some(c))
        .await
        .unwrap();
    assert_eq!(access.agent_instance_id, a);
    assert_eq!(access.workspace_id, w);
    assert_eq!(
        access.agent_folder,
        fx.afm.get(a).await.unwrap().folder_path
    );
    let find = |id: PlaceId| access.grants.iter().find(|g| g.place_id == id).cloned();
    assert_eq!(find(p).unwrap().access, FolderAccess::Read);
    assert_eq!(find(p).unwrap().path, place_p);
    assert_eq!(find(q).unwrap().access, FolderAccess::ReadAndChange);
    assert_eq!(access.grants.len(), 2);
}

#[tokio::test]
async fn edge_a_grant_on_another_workspace_is_refused() {
    let fx = fixture().await;
    let (a, w, v) = (instance(5), WorkspaceId::new(), WorkspaceId::new());
    let place = fx.dir.dir("kept");
    let kept = grant_for(
        w,
        to_instance(a),
        PlaceId::new(),
        FolderAccess::Read,
        &place,
    );
    fx.afm.set_workspace_grants(w, vec![kept]).await.unwrap();
    let foreign = grant_for(
        v,
        to_instance(a),
        PlaceId::new(),
        FolderAccess::Read,
        &place,
    );
    let foreign_id = foreign.grant.id;
    let error = fx
        .afm
        .set_workspace_grants(w, vec![foreign])
        .await
        .unwrap_err();
    assert_eq!(
        error,
        AfmError::GrantInvalid {
            folder_grant_id: foreign_id
        }
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM workspace_folder_grants")
            .await,
        1,
        "the stored set of W is unchanged"
    );
}

#[tokio::test]
async fn error_install_while_a_session_runs() {
    assert_eq!(WRITE_LOCK_WAIT, Duration::from_secs(30));
    // The test waits 300 ms where the spec waits 30 s; the constant is asserted above.
    let fx = fixture_waiting(Duration::from_millis(300)).await;
    let id = instance(6);
    fx.afm.create(id).await.unwrap();
    let _session = fx.afm.lock(id, LockMode::Session).await.unwrap();
    let started = Instant::now();
    let error = fx.afm.lock(id, LockMode::Write).await.unwrap_err();
    assert_eq!(
        error,
        AfmError::FolderBusy {
            id,
            held: "session"
        }
    );
    assert!(started.elapsed() >= Duration::from_millis(250));
}

#[tokio::test]
async fn error_interrupted_removal_is_finished_on_the_next_start() {
    let fx = fixture().await;
    let id = instance(7);
    let folder = fx.afm.create(id).await.unwrap();
    std::fs::write(folder.folder_path.join("work.txt"), b"x").unwrap();
    fx.sql("UPDATE agent_folders SET state = 'removing'").await;
    assert!(folder.folder_path.exists());

    let report = fx.afm.recover().await.unwrap();
    assert_eq!(report.removals_finished, 1);
    assert!(!folder.folder_path.exists() && !folder.meta_path.exists());
    assert_eq!(
        fx.count("SELECT count(*) FROM agent_folders WHERE state = 'removed'")
            .await,
        1
    );
}

#[tokio::test]
async fn error_unwritable_data_root() {
    let dir = TempDir::new();
    let journal = super::support::journal_in(&dir).await;
    let root = dir.dir("locked");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    #[cfg(not(unix))]
    let root = {
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        file.join("data")
    };
    let error = AgentFolderManager::new(journal, root).await.err().unwrap();
    assert_eq!(error, AfmError::RootUnwritable);
}

// spec: ordin-desktop/components/AgentFolderManager#AFM-PATH-1
#[cfg(windows)]
#[tokio::test]
async fn happy_path_a_granted_folder_is_applied_in_its_plain_form_on_windows() {
    let fx = fixture().await;
    let (a, w, p) = (instance(20), WorkspaceId::new(), PlaceId::new());
    fx.afm.create(a).await.unwrap();
    // The folder in its plain form, and the same folder as the operating system resolves it.
    let plain = fx.dir.dir("project");
    let verbatim = std::path::PathBuf::from(format!(r"\\?\{}", plain.display()));
    assert!(!plain.to_string_lossy().starts_with(r"\\?\"));
    let grant = grant_for(w, to_instance(a), p, FolderAccess::Read, &verbatim);
    fx.afm.set_workspace_grants(w, vec![grant]).await.unwrap();
    let access = fx.afm.apply_grants(RunId::new(), a, w, None).await.unwrap();
    assert_eq!(access.grants.len(), 1);
    let granted = &access.grants[0].path;
    assert!(
        !granted.to_string_lossy().starts_with(r"\\?\"),
        "{}",
        granted.display()
    );
    assert_eq!(granted, &plain);
}
