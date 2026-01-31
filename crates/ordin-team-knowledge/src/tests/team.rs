//! Gherkin "Feature: Team Knowledge Service — a team workspace (ADR-121)" and its Error Table rows.

use ordin_push::PushNotice;
use ordin_types::domain::{FolderAccess, SourceLinkKind};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::RunId;
use ordin_workspaces::Reach;

use super::support::*;
use crate::files::{self, TeamWorkspaceFile};
use crate::model::*;
use crate::store::ShareScope;

const CONTRACTS: &str = "graph:drives/b!x9Fq2k/items/01ABCDEF2GHIJ";

fn labels(p: &ordin_push::TeamShareProposal) -> Vec<String> {
    p.card
        .affects
        .iter()
        .filter_map(|a| match a {
            AffectedItem::Record { label } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

/// Background: W "Q3 supplier review" with an active SharePoint link "Contracts", a local folder link
/// "Notes" and the agent "Supplier analyst" holding read and change on "Contracts".
async fn background() -> (Rig, ordin_types::AgentInstanceId) {
    let rig = Rig::new().await;
    let contracts = rig.workspaces.link(
        rig.ws,
        SourceLinkKind::SharepointLocation,
        CONTRACTS,
        "Contracts",
    );
    rig.workspaces.link(
        rig.ws,
        SourceLinkKind::LocalFolder,
        "/Users/asha/Notes",
        "Notes",
    );
    let analyst = rig.agents.put(
        rig.ws,
        "Supplier analyst",
        "urn:ordin:acme:workspaces:supplier-analyst",
    );
    rig.workspaces.grant(
        rig.ws,
        analyst,
        contracts.place_id.unwrap(),
        FolderAccess::ReadAndChange,
    );
    (rig, analyst)
}

/// Shares W as a team workspace and approves the card.
async fn shared() -> (
    Rig,
    ordin_types::AgentInstanceId,
    ordin_push::TeamShareProposal,
) {
    let (rig, analyst) = background().await;
    let status = rig
        .service
        .share_workspace(rig.ws, rig.choice(), TeamResultsChoice::TeamFolder {})
        .await
        .expect("share");
    assert_eq!(status.busy, Some(TeamBusy::Sharing));
    rig.wait_idle().await;
    let p = rig.push.last();
    rig.approve(&p).await;
    (rig, analyst, p)
}

fn description(host: &Host) -> TeamWorkspaceFile {
    files::parse_description(&host.file(files::DESCRIPTION_FILE).expect("description"))
        .expect("readable")
}

/// Priya's computer opens the team workspace W shared from Asha's.
async fn opened_by_priya(asha: &Rig) -> (Rig, ordin_types::WorkspaceId) {
    let priya = asha.colleague("priya-shah", "Priya Shah").await;
    priya
        .agents
        .library
        .lock()
        .unwrap()
        .insert("urn:ordin:acme:workspaces:supplier-analyst".into());
    let opened = priya
        .service
        .open_workspace(
            asha.host.url(),
            "main".into(),
            FOLDER.into(),
            "Supplier review".into(),
        )
        .await
        .expect("open");
    let ws = opened.workspace_id;
    for _ in 0..400 {
        if priya.service.busy(ws).is_none() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    (priya, ws)
}

// Scenario: Happy path — sharing a workspace as a team workspace
#[tokio::test]
async fn happy_path_sharing_a_workspace_as_a_team_workspace() {
    let (rig, analyst, p) = shared().await;
    assert_eq!(
        p.card.question,
        "Share Q3 supplier review as a team workspace with acme/supplier-team?"
    );
    let l = labels(&p);
    assert!(l.contains(&"Places: Contracts".to_owned()), "{l:?}");
    assert!(
        l.contains(&"Stays on this computer: Notes".to_owned()),
        "{l:?}"
    );
    assert!(
        l.contains(&"Agents: Supplier analyst (can read and change Contracts)".to_owned()),
        "{l:?}"
    );
    let d = description(&rig.host);
    assert_eq!(d.places.len(), 1);
    assert_eq!(d.places[0].display_name, "Contracts");
    let text = String::from_utf8(rig.host.file(files::DESCRIPTION_FILE).unwrap()).unwrap();
    assert!(!text.contains('@') && !text.contains("token") && !text.contains("password"));
    let results = rig.workspaces.state.lock().unwrap().results_folder[&rig.ws];
    assert!(rig
        .workspaces
        .grants_of(analyst)
        .contains(&(results, FolderAccess::ReadAndChange)));
    let status = rig.status().await;
    assert_eq!(
        status.team_workspace.unwrap().state,
        TeamWorkspaceState::Team
    );
    assert!(rig
        .journal_kinds()
        .await
        .contains(&"team_knowledge_workspace_shared".to_owned()));
}

// Scenario: Edge — older installations ignore the new files
#[tokio::test]
async fn edge_older_installations_ignore_the_new_files() {
    for path in [
        files::DESCRIPTION_FILE,
        "activity/0192c2a0-7b1c-7d2e-8f30-4a5b6c7d8e9f.json",
        "results/Board pack.md",
    ] {
        let kind = files::classify(path).expect("recognised by this build");
        assert!(
            !matches!(kind, files::PathKind::Item { .. }),
            "{path} is never an item"
        );
    }
    // A get of a workspace that is not a team workspace records no problem for any of the three.
    let (asha, _, _) = shared().await;
    let reader = asha.colleague("sam-okafor", "Sam Okafor").await;
    reader.on().await;
    reader.get().await.unwrap();
    assert!(reader.status().await.problems.is_empty());
}

// Scenario: Happy path — a colleague opens the team workspace
#[tokio::test]
async fn happy_path_a_colleague_opens_the_team_workspace() {
    let (asha, _, _) = shared().await;
    let priya = asha.colleague("priya-shah", "Priya Shah").await;
    priya
        .agents
        .library
        .lock()
        .unwrap()
        .insert("urn:ordin:acme:workspaces:supplier-analyst".into());
    let preview = priya
        .service
        .workspace_open_preview(asha.host.url(), "main".into(), FOLDER.into())
        .await
        .unwrap();
    assert_eq!(preview.places[0].display_name, "Contracts");
    assert_eq!(preview.places[0].reach, Some(TeamReach::WillLink));
    assert_eq!(preview.agents[0].found, Some(TeamAgentFound::InYourLibrary));
    assert!(preview.agents[0]
        .access
        .iter()
        .any(|a| a.place_name == "Contracts" && a.access == FolderAccess::ReadAndChange));
    let (priya, ws) = opened_by_priya(&asha).await;
    let links = priya.workspaces.links_of(ws);
    let contracts = links
        .iter()
        .find(|l| l.display_name == "Contracts")
        .expect("Contracts linked");
    let agent = priya
        .agents
        .in_workspace(ws, "urn:ordin:acme:workspaces:supplier-analyst")
        .expect("added");
    assert!(priya
        .workspaces
        .grants_of(agent)
        .contains(&(contracts.place_id.unwrap(), FolderAccess::ReadAndChange)));
    assert!(
        priya.service.status(ws).await.unwrap().last_get.is_some(),
        "a get was started"
    );
}

// Scenario: Edge — a place this person cannot open is left out
#[tokio::test]
async fn edge_a_place_this_person_cannot_open_is_left_out() {
    let (asha, _, _) = shared().await;
    let sam = asha.colleague("sam-okafor", "Sam Okafor").await;
    sam.workspaces
        .state
        .lock()
        .unwrap()
        .reach
        .insert(CONTRACTS.into(), Reach::NoAccess);
    sam.workspaces.state.lock().unwrap().link_refused.insert(
        CONTRACTS.into(),
        ordin_workspaces::WorkspaceError::SourceAccessDenied,
    );
    sam.agents
        .library
        .lock()
        .unwrap()
        .insert("urn:ordin:acme:workspaces:supplier-analyst".into());
    let preview = sam
        .service
        .workspace_open_preview(asha.host.url(), "main".into(), FOLDER.into())
        .await
        .unwrap();
    assert_eq!(preview.places[0].reach, Some(TeamReach::NoAccess));
    let opened = sam
        .service
        .open_workspace(asha.host.url(), "main".into(), FOLDER.into(), "Q3".into())
        .await
        .unwrap();
    let ws = opened.workspace_id;
    assert!(sam
        .workspaces
        .links_of(ws)
        .iter()
        .all(|l| l.display_name != "Contracts"));
    let agent = sam
        .agents
        .in_workspace(ws, "urn:ordin:acme:workspaces:supplier-analyst")
        .unwrap();
    let results = sam.workspaces.state.lock().unwrap().results_folder[&ws];
    assert!(
        sam.workspaces
            .grants_of(agent)
            .iter()
            .all(|(p, _)| *p == results),
        "no access to Contracts"
    );
    let tw = opened.status.team_workspace.unwrap();
    assert!(tw
        .places
        .iter()
        .any(|p| p.display_name == "Contracts" && p.state == TeamPlaceState::NoAccess));
}

// Scenario: Happy path — a run adds one activity entry with no conversation text
#[tokio::test]
async fn happy_path_a_run_adds_one_activity_entry_with_no_conversation_text() {
    let (rig, analyst, _) = shared().await;
    let run = RunId::new();
    rig.workspaces.write_result(
        rig.ws,
        "Kestrel quote — comparison.md",
        b"# Comparison\n",
        Some(run),
    );
    rig.runs
        .runs
        .lock()
        .unwrap()
        .insert(run, run_facts(rig.ws, analyst, "Compare documents"));
    rig.service.record_run(run).await;
    let p = rig.push.last();
    assert_eq!(p.card.question, "Share 1 result with acme/supplier-team?");
    rig.approve(&p).await;
    let changed = changed_paths(&rig.host, rig.host.tip());
    assert!(changed.contains(&(
        format!("{FOLDER}/results/Kestrel quote — comparison.md"),
        false
    )));
    let activity: Vec<_> = rig
        .host
        .files()
        .into_keys()
        .filter(|p| p.starts_with("activity/"))
        .collect();
    assert_eq!(activity.len(), 1);
    let stem = activity[0]
        .trim_start_matches("activity/")
        .trim_end_matches(".json")
        .to_owned();
    let entry = files::parse_activity(&rig.host.file(&activity[0]).unwrap(), &stem).unwrap();
    assert_eq!(entry.by, "asha-patel");
    assert_eq!(entry.task.as_deref(), Some("Compare documents"));
    assert_eq!(entry.agent.as_deref(), Some("Supplier analyst"));
    assert_eq!(
        entry.results,
        vec!["Kestrel quote — comparison.md".to_owned()]
    );
    // Only the file's fields: no arguments, conversation or approval.
    let value: serde_json::Value =
        serde_json::from_slice(&rig.host.file(&activity[0]).unwrap()).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 12);
    // The first build writes no log line (build note).
    assert!(rig.host.file("log.md").is_none() && rig.host.file("index.md").is_none());
}

// Scenario: Security — a colleague's added agent waits for this person
#[tokio::test]
async fn security_a_colleagues_added_agent_waits_for_this_person() {
    let (asha, _, _) = shared().await;
    let (priya, pws) = opened_by_priya(&asha).await;
    // Priya adds "Contract checker" with read and change on Contracts, and shares.
    let contracts = priya
        .workspaces
        .links_of(pws)
        .into_iter()
        .find(|l| l.display_name == "Contracts")
        .unwrap();
    let checker = priya.agents.put(
        pws,
        "Contract checker",
        "urn:ordin:acme:workspaces:contract-checker",
    );
    priya.workspaces.grant(
        pws,
        checker,
        contracts.place_id.unwrap(),
        FolderAccess::ReadAndChange,
    );
    let priya_rig = Rig { ws: pws, ..priya };
    let p = priya_rig.share().await.unwrap();
    priya_rig.approve(&p).await;

    asha.agents
        .library
        .lock()
        .unwrap()
        .insert("urn:ordin:acme:workspaces:contract-checker".into());
    asha.get().await.unwrap();
    assert!(
        asha.agents
            .in_workspace(asha.ws, "urn:ordin:acme:workspaces:contract-checker")
            .is_none(),
        "not added"
    );
    let grants = asha.service.pending_grants(asha.ws).await.unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].added_by, "priya-shah");
    assert!(
        grants[0].added_by_confirmed,
        "Priya's share names her login in its author"
    );
    assert!(grants[0].new_agent);
    assert!(grants[0]
        .access
        .iter()
        .any(|a| a.place_name == "Contracts" && a.access == FolderAccess::ReadAndChange));
    asha.service
        .allow_grant(asha.ws, grants[0].grant_id)
        .await
        .unwrap();
    let added = asha
        .agents
        .in_workspace(asha.ws, "urn:ordin:acme:workspaces:contract-checker")
        .expect("added");
    let place = asha
        .workspaces
        .links_of(asha.ws)
        .into_iter()
        .find(|l| l.display_name == "Contracts")
        .unwrap()
        .place_id
        .unwrap();
    assert!(asha
        .workspaces
        .grants_of(added)
        .contains(&(place, FolderAccess::ReadAndChange)));
    assert_eq!(
        asha.service
            .allow_grant(asha.ws, grants[0].grant_id)
            .await
            .unwrap_err()
            .code,
        "TEAM_WS_GRANT_NOT_FOUND"
    );
}

// Scenario: Edge — declined access is not asked again for the same change
#[tokio::test]
async fn edge_declined_access_is_not_asked_again_for_the_same_change() {
    let (asha, _, _) = shared().await;
    let (priya, pws) = opened_by_priya(&asha).await;
    let checker = priya.agents.put(
        pws,
        "Contract checker",
        "urn:ordin:acme:workspaces:contract-checker",
    );
    let results = priya.workspaces.state.lock().unwrap().results_folder[&pws];
    priya
        .workspaces
        .grant(pws, checker, results, FolderAccess::ReadAndChange);
    let priya = Rig { ws: pws, ..priya };
    let p = priya.share().await.unwrap();
    priya.approve(&p).await;
    asha.get().await.unwrap();
    let g = asha.service.pending_grants(asha.ws).await.unwrap();
    asha.service
        .decline_grant(asha.ws, g[0].grant_id)
        .await
        .unwrap();
    // A later get with no change to that agent's setting asks nothing.
    asha.host
        .commit(&[("README-team.md", Some(b"x"))], "Someone");
    asha.get().await.unwrap();
    assert!(asha
        .service
        .pending_grants(asha.ws)
        .await
        .unwrap()
        .is_empty());
}

// Scenario: Edge — removals apply on get
#[tokio::test]
async fn edge_removals_apply_on_get() {
    let (asha, _, _) = shared().await;
    let mut d = description(&asha.host);
    d.places.clear();
    for a in d.agents.iter_mut() {
        a.access
            .retain(|e| e.place == "results" || e.place == "own");
    }
    asha.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya Shah (priya-shah)",
    );
    asha.get().await.unwrap();
    assert!(asha
        .workspaces
        .links_of(asha.ws)
        .iter()
        .all(|l| l.display_name != "Contracts"));
}

// Scenario: Edge — a removal waits for unsent changes
#[tokio::test]
async fn edge_a_removal_waits_for_unsent_changes() {
    let (asha, _, _) = shared().await;
    let contracts = asha
        .workspaces
        .links_of(asha.ws)
        .into_iter()
        .find(|l| l.display_name == "Contracts")
        .unwrap();
    asha.workspaces
        .state
        .lock()
        .unwrap()
        .changed_copies
        .insert(contracts.id);
    let mut d = description(&asha.host);
    d.places.clear();
    for a in d.agents.iter_mut() {
        a.access
            .retain(|e| e.place == "results" || e.place == "own");
    }
    asha.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya Shah (priya-shah)",
    );
    asha.get().await.unwrap();
    assert!(asha
        .workspaces
        .links_of(asha.ws)
        .iter()
        .any(|l| l.id == contracts.id));
    let tw = asha.status().await.team_workspace.unwrap();
    assert!(tw
        .places
        .iter()
        .any(|p| p.state == TeamPlaceState::RemovalWaits));
}

// Scenario: Edge — two changes to one setting: the later share wins
#[tokio::test]
async fn edge_two_changes_to_one_setting_the_later_share_wins() {
    let (asha, analyst, _) = shared().await;
    // Priya changes Supplier analyst's access to read only and shares first.
    let mut d = description(&asha.host);
    let key = d.places[0].key.clone();
    for e in d.agents[0].access.iter_mut().filter(|e| e.place == key) {
        e.access = "read".into();
    }
    d.agents[0].changed_by = "priya-shah".into();
    d.agents[0].changed_at = ordin_types::time::Timestamp::now();
    asha.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya Shah (priya-shah)",
    );
    // Asha removed that access here since the last sync.
    let place = asha
        .workspaces
        .links_of(asha.ws)
        .into_iter()
        .find(|l| l.display_name == "Contracts")
        .unwrap()
        .place_id
        .unwrap();
    asha.workspaces
        .state
        .lock()
        .unwrap()
        .grants
        .retain(|g| g.place_id != place);
    asha.get().await.unwrap();
    let page = asha.service.activity(asha.ws, None).await.unwrap();
    let replaced = page
        .items
        .iter()
        .find(|i| i.kind == TeamActivityKind::Replaced)
        .expect("a replaced entry");
    assert_eq!(replaced.by.as_deref(), Some("priya-shah"));
    assert_eq!(replaced.replaced_by.as_deref(), Some("asha-patel"));
    let p = asha.share().await.unwrap();
    asha.approve(&p).await;
    let after = description(&asha.host);
    assert!(
        after.agents[0].access.iter().all(|e| e.place != key),
        "Asha's access holds"
    );
    let _ = analyst;
}

// Scenario: Happy path — Team activity, newest first
#[tokio::test]
async fn happy_path_team_activity_newest_first() {
    let (rig, analyst, _) = shared().await;
    for (n, at) in [
        "2026-01-20T09:00:00.000Z",
        "2026-01-20T11:00:00.000Z",
        "2026-01-20T10:00:00.000Z",
    ]
    .iter()
    .enumerate()
    {
        let run = RunId::new();
        rig.workspaces
            .write_result(rig.ws, &format!("r{n}.md"), b"x", Some(run));
        let mut f = run_facts(rig.ws, analyst, "Compare documents");
        f.ended_at = ts(at);
        rig.runs.runs.lock().unwrap().insert(run, f);
        // An open share holds later results back for the next share.
        rig.service.record_run(run).await;
    }
    let page = rig.service.activity(rig.ws, None).await.unwrap();
    let times: Vec<String> = page.items.iter().map(|i| i.at.to_wire()).collect();
    assert_eq!(
        times,
        [
            "2026-01-20T11:00:00.000Z",
            "2026-01-20T10:00:00.000Z",
            "2026-01-20T09:00:00.000Z"
        ]
    );
    let results = rig.workspaces.state.lock().unwrap().results_folder[&rig.ws];
    assert!(
        matches!(page.items[0].results[0].file, Some(ordin_types::wire::approval::ContentRef::PlaceFile { place_id, .. }) if place_id == results)
    );
}

// Scenario: Happy path — leaving keeps everything here and changes nothing in the folder
#[tokio::test]
async fn happy_path_leaving_keeps_everything_here_and_changes_nothing_in_the_folder() {
    let (rig, analyst, _) = shared().await;
    let links = rig.workspaces.links_of(rig.ws).len();
    let grants = rig.workspaces.grants_of(analyst).len();
    let folder = rig.host.files();
    rig.service.leave_workspace(rig.ws).await.unwrap();
    assert_eq!(rig.workspaces.links_of(rig.ws).len(), links);
    assert_eq!(rig.workspaces.grants_of(analyst).len(), grants);
    assert_eq!(rig.host.files(), folder);
    assert!(rig.status().await.team_workspace.is_none());
    assert!(rig
        .journal_kinds()
        .await
        .contains(&"team_knowledge_workspace_left".to_owned()));
    assert_eq!(
        rig.service.leave_workspace(rig.ws).await.unwrap_err().code,
        "TEAM_WS_NOT_TEAM"
    );
}

// Scenario: Error — opening a folder with no team workspace
#[tokio::test]
async fn error_opening_a_folder_with_no_team_workspace() {
    let rig = Rig::new().await;
    let e = rig
        .service
        .workspace_open_preview(rig.host.url(), "main".into(), "notes".into())
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_NOT_A_TEAM_WORKSPACE");
    let before = rig.workspaces.state.lock().unwrap().workspaces.len();
    let e = rig
        .service
        .open_workspace(
            rig.host.url(),
            "main".into(),
            "notes".into(),
            "Notes".into(),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_NOT_A_TEAM_WORKSPACE");
    assert_eq!(
        rig.workspaces.state.lock().unwrap().workspaces.len(),
        before,
        "no workspace is created"
    );
}

// Scenario: Error — a newer description format
#[tokio::test]
async fn error_a_newer_description_format() {
    let (rig, _, _) = shared().await;
    let mut value: serde_json::Value =
        serde_json::from_slice(&rig.host.file(files::DESCRIPTION_FILE).unwrap()).unwrap();
    value["format"] = serde_json::json!(2);
    rig.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&value)))],
        "Priya",
    );
    let other = rig.colleague("sam-okafor", "Sam").await;
    let e = other
        .service
        .workspace_open_preview(rig.host.url(), "main".into(), FOLDER.into())
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_FORMAT_NEWER");
    // At a get it is a problem and nothing is applied.
    rig.get().await.unwrap();
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_FORMAT_NEWER"));
}

// Scenario: Error — a description carrying a credential
#[tokio::test]
async fn error_a_description_carrying_a_credential() {
    let (rig, _, _) = shared().await;
    let mut d = description(&rig.host);
    d.places[0].kind = "git_remote".into();
    d.places[0].location = "git:https://x:secret@github.com/acme/a#main:".into();
    rig.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya",
    );
    let other = rig.colleague("sam-okafor", "Sam").await;
    let before = other.workspaces.state.lock().unwrap().workspaces.len();
    let e = other
        .service
        .workspace_open_preview(rig.host.url(), "main".into(), FOLDER.into())
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_DESCRIPTION_UNREADABLE");
    assert_eq!(
        other.workspaces.state.lock().unwrap().workspaces.len(),
        before
    );
}

// Scenario: Happy path — the results grant is made before the description is rendered (TW-A)
#[tokio::test]
async fn happy_path_the_results_grant_is_made_before_the_description_is_rendered() {
    let (rig, analyst) = background().await;
    rig.service
        .share_workspace(rig.ws, rig.choice(), TeamResultsChoice::TeamFolder {})
        .await
        .unwrap();
    rig.wait_idle().await;
    let results = rig.workspaces.state.lock().unwrap().results_folder[&rig.ws];
    assert!(
        rig.workspaces
            .grants_of(analyst)
            .contains(&(results, FolderAccess::ReadAndChange)),
        "before the card is raised"
    );
    let p = rig.push.last();
    let repo = git2::Repository::open_bare(&p.clone_path).unwrap();
    let commit = git2::Oid::from_str(&p.commit).unwrap();
    let blob = crate::git::blob_at(&repo, commit, FOLDER, files::DESCRIPTION_FILE).unwrap();
    let d = files::parse_description(repo.find_blob(blob).unwrap().content()).unwrap();
    assert!(d.agents[0]
        .access
        .iter()
        .any(|e| e.place == "results" && e.access == "read_and_change"));
    rig.service
        .on_notice(PushNotice::TeamShareRejected {
            share_id: p.share_id,
        })
        .await;
    assert!(
        !rig.workspaces
            .grants_of(analyst)
            .iter()
            .any(|(p, _)| *p == results),
        "no grant on the results folder"
    );
    assert!(rig.status().await.team_workspace.is_none());
    assert!(
        rig.status().await.binding.is_none(),
        "turned on by the share, so turned off again"
    );
}

// Scenario: Happy path — a Confluence space link is shared and opened (SA-TEAM-TW-06)
#[tokio::test]
async fn happy_path_a_confluence_space_link_is_shared_and_opened() {
    let (rig, _) = background().await;
    rig.workspaces.link(
        rig.ws,
        SourceLinkKind::ConfluenceLocation,
        "confluence:space/PROC",
        "Procurement",
    );
    rig.service
        .share_workspace(rig.ws, rig.choice(), TeamResultsChoice::TeamFolder {})
        .await
        .unwrap();
    rig.wait_idle().await;
    rig.approve(&rig.push.last()).await;
    let d = description(&rig.host);
    assert!(d
        .places
        .iter()
        .any(|p| p.kind == "confluence" && p.location == "confluence:space/PROC"));
    let priya = rig.colleague("priya-shah", "Priya").await;
    assert!(priya
        .service
        .workspace_open_preview(rig.host.url(), "main".into(), FOLDER.into())
        .await
        .is_ok());
}

// Scenario: Security — a name the history does not record says so (TW-J)
#[tokio::test]
async fn security_a_name_the_history_does_not_record_says_so() {
    let (rig, _, _) = shared().await;
    let id = ordin_types::TeamActivityId::new();
    let entry = files::TeamActivityFile {
        format: 1,
        id,
        kind: "run".into(),
        by: "priya-shah".into(),
        at: ordin_types::time::Timestamp::now(),
        task: Some("Compare documents".into()),
        agent: Some("Supplier analyst".into()),
        outcome: Some("completed".into()),
        results: vec![],
        change: None,
        subject: None,
        replaced_by: None,
    };
    rig.host.commit(
        &[(&format!("activity/{id}.json"), Some(&files::render(&entry)))],
        "Sam Okafor",
    );
    rig.get().await.unwrap();
    let page = rig.service.activity(rig.ws, None).await.unwrap();
    let got = page.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(got.by.as_deref(), Some("priya-shah"));
    assert!(!got.by_confirmed);
}

// Scenario: Happy path — a name written by ordin agrees (TW-J)
#[tokio::test]
async fn happy_path_a_name_written_by_ordin_agrees() {
    let (rig, _, _) = shared().await;
    let id = ordin_types::TeamActivityId::new();
    let entry = files::TeamActivityFile {
        format: 1,
        id,
        kind: "run".into(),
        by: "priya-shah".into(),
        at: ordin_types::time::Timestamp::now(),
        task: None,
        agent: Some("Supplier analyst".into()),
        outcome: Some("failed".into()),
        results: vec![],
        change: None,
        subject: None,
        replaced_by: None,
    };
    rig.host.commit(
        &[(&format!("activity/{id}.json"), Some(&files::render(&entry)))],
        "Priya Shah (priya-shah)",
    );
    rig.get().await.unwrap();
    let page = rig.service.activity(rig.ws, None).await.unwrap();
    assert!(page.items.iter().find(|i| i.id == id).unwrap().by_confirmed);
}

// Scenario: Edge — two results of the same name are both kept (TW-G)
#[tokio::test]
async fn edge_two_results_of_the_same_name_are_both_kept() {
    let (rig, _, _) = shared().await;
    rig.workspaces
        .write_result(rig.ws, "Board pack.md", b"Asha's\n", None);
    rig.host.commit(
        &[("results/Board pack.md", Some(b"Priya's\n"))],
        "Priya Shah (priya-shah)",
    );
    rig.get().await.unwrap();
    assert_eq!(
        rig.workspaces.result(rig.ws, "Board pack.md").unwrap(),
        b"Priya's\n"
    );
    assert_eq!(
        rig.workspaces
            .result(rig.ws, "Board pack (asha-patel).md")
            .unwrap(),
        b"Asha's\n"
    );
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_RESULT_KEPT"));
    let p = rig.share().await.unwrap();
    rig.approve(&p).await;
    let changed = changed_paths(&rig.host, rig.host.tip());
    assert!(changed.contains(&(
        format!("{FOLDER}/results/Board pack (asha-patel).md"),
        false
    )));
    assert!(!changed
        .iter()
        .any(|(p, _)| p == &format!("{FOLDER}/results/Board pack.md")));
}

// Scenario: Error — a result path the rule refuses (SA-TEAM-TW-13)
#[tokio::test]
async fn error_a_result_path_the_rule_refuses() {
    let (rig, _, _) = shared().await;
    rig.host
        .commit(&[("results/C:\\temp\\x.md", Some(b"x"))], "Priya");
    rig.get().await.unwrap();
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.path == "results/C:\\temp\\x.md" && p.code == "TEAM_FILE_UNREADABLE"));
    assert!(
        rig.workspaces.result(rig.ws, "C:\\temp\\x.md").is_none(),
        "nothing is stored"
    );
}

// Scenario: Edge — a commit from a team workspace names the login (TW-B)
#[tokio::test]
async fn edge_a_commit_from_a_team_workspace_names_the_login() {
    let (rig, _, _) = shared().await;
    let host_repo = rig.host.repo();
    let c = host_repo.find_commit(rig.host.tip()).unwrap();
    assert_eq!(c.author().name().ok(), Some("Asha Patel (asha-patel)"));
}

// Scenario: Error — read-only access keeps results here
#[tokio::test]
async fn error_read_only_access_keeps_results_here() {
    let (asha, _, _) = shared().await;
    let (sam, sws) = opened_by_priya(&asha).await;
    *sam.repos.permission.lock().unwrap() = Ok(ordin_projects::RepoPermission::Read);
    let sam = Rig { ws: sws, ..sam };
    // Access is read again at the next check.
    let pool = sam.journal.datastore().sqlite().unwrap().clone();
    sqlx::query("UPDATE team_knowledge_bindings SET access = 'read_only'")
        .execute(&pool)
        .await
        .unwrap();
    let agent = sam
        .agents
        .in_workspace(sws, "urn:ordin:acme:workspaces:supplier-analyst")
        .unwrap();
    let run = RunId::new();
    sam.workspaces.write_result(sws, "Mine.md", b"x", Some(run));
    sam.runs
        .runs
        .lock()
        .unwrap()
        .insert(run, run_facts(sws, agent, "Compare documents"));
    let before = sam.push.count();
    sam.service.record_run(run).await;
    assert_eq!(sam.push.count(), before, "no card is raised");
    assert!(sam.status().await.team_workspace.unwrap().results_here_only);
}

// ---- Error Table rows of the amendment ----

#[tokio::test]
async fn error_row_already_a_team_workspace_and_folder_taken() {
    let (rig, _, _) = shared().await;
    let e = rig
        .service
        .workspace_share_preview(rig.ws, rig.choice())
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_ALREADY_TEAM");
    // Another workspace bound to the same folder finds it taken.
    let other = rig.colleague("sam-okafor", "Sam").await;
    let e = other
        .service
        .workspace_share_preview(other.ws, other.choice())
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_FOLDER_TAKEN");
}

#[tokio::test]
async fn error_row_already_open_and_name_in_use() {
    let (asha, _, _) = shared().await;
    let (priya, _) = opened_by_priya(&asha).await;
    let e = priya
        .service
        .workspace_open_preview(asha.host.url(), "main".into(), FOLDER.into())
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_ALREADY_OPEN");
    let fresh = asha.colleague("sam-okafor", "Sam").await;
    let e = fresh
        .service
        .open_workspace(
            asha.host.url(),
            "main".into(),
            FOLDER.into(),
            "Q3 supplier review".into(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        e.code, "TEAM_WS_NAME_IN_USE",
        "Sam already has a workspace of that name"
    );
}

#[tokio::test]
async fn error_row_results_not_suitable() {
    let (rig, _) = background().await;
    let notes = rig
        .workspaces
        .links_of(rig.ws)
        .into_iter()
        .find(|l| l.display_name == "Notes")
        .unwrap();
    let e = rig
        .service
        .share_workspace(
            rig.ws,
            rig.choice(),
            TeamResultsChoice::Link {
                source_link_id: notes.id,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_RESULTS_NOT_SUITABLE");
}

#[tokio::test]
async fn error_row_not_a_team_workspace() {
    let rig = Rig::new().await;
    rig.on().await;
    assert_eq!(
        rig.service.activity(rig.ws, None).await.unwrap_err().code,
        "TEAM_WS_NOT_TEAM"
    );
    assert_eq!(
        rig.service
            .allow_grant(rig.ws, ordin_types::TeamGrantId::new())
            .await
            .unwrap_err()
            .code,
        "TEAM_WS_NOT_TEAM"
    );
    assert_eq!(
        rig.service
            .decline_grant(rig.ws, ordin_types::TeamGrantId::new())
            .await
            .unwrap_err()
            .code,
        "TEAM_WS_NOT_TEAM"
    );
}

#[tokio::test]
async fn error_row_the_allowed_access_cannot_be_given() {
    let (rig, _) = background().await;
    rig.workspaces.state.lock().unwrap().fail_set_grant = true;
    rig.service
        .share_workspace(rig.ws, rig.choice(), TeamResultsChoice::TeamFolder {})
        .await
        .unwrap();
    rig.wait_idle().await;
    assert_eq!(rig.status().await.last_error, Some("TEAM_WS_ALLOW_FAILED"));
    assert_eq!(rig.push.count(), 0, "nothing is proposed");
}

#[tokio::test]
async fn error_row_an_agent_of_a_grant_cannot_be_added() {
    let (asha, _, _) = shared().await;
    let mut d = description(&asha.host);
    let mut agent = d.agents[0].clone();
    agent.key = uuid::Uuid::now_v7().hyphenated().to_string();
    agent.id = "urn:ordin:acme:workspaces:unknown".into();
    agent.name = "Unknown".into();
    agent.changed_by = "priya-shah".into();
    d.agents.push(agent);
    d.agents.sort_by(|a, b| a.key.cmp(&b.key));
    asha.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya Shah (priya-shah)",
    );
    asha.get().await.unwrap();
    let g = asha.service.pending_grants(asha.ws).await.unwrap();
    let e = asha
        .service
        .allow_grant(asha.ws, g[0].grant_id)
        .await
        .unwrap_err();
    assert_eq!(e.code, "TEAM_WS_AGENT_NOT_ADDED");
    assert_eq!(
        asha.service.pending_grants(asha.ws).await.unwrap().len(),
        1,
        "the grant stays waiting"
    );
}

#[tokio::test]
async fn error_row_a_result_larger_than_50_mib_stays_here() {
    let (rig, analyst, _) = shared().await;
    let run = RunId::new();
    rig.workspaces.write_result(
        rig.ws,
        "Huge.bin",
        &vec![0u8; (files::TEAM_RESULT_MAX_BYTES + 1) as usize],
        Some(run),
    );
    rig.runs
        .runs
        .lock()
        .unwrap()
        .insert(run, run_facts(rig.ws, analyst, "Compare documents"));
    rig.service.record_run(run).await;
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_RESULT_TOO_LARGE"));
}

#[tokio::test]
async fn error_row_a_colleagues_new_name_is_taken() {
    let (rig, _, _) = shared().await;
    rig.workspaces.workspace("Taken name");
    let mut d = description(&rig.host);
    d.name = "Taken name".into();
    d.name_changed_by = "priya-shah".into();
    d.name_changed_at = ordin_types::time::Timestamp::now();
    rig.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya Shah (priya-shah)",
    );
    rig.get().await.unwrap();
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_NAME_IN_USE"));
    assert_eq!(rig.workspaces.name_of(rig.ws), "Q3 supplier review");
}

#[tokio::test]
async fn error_row_an_unreadable_description_at_get() {
    let (rig, _, _) = shared().await;
    rig.host.commit(
        &[(
            files::DESCRIPTION_FILE,
            Some(b"{ \"format\": 1, \"oops\": true }"),
        )],
        "Priya",
    );
    rig.get().await.unwrap();
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_DESCRIPTION_UNREADABLE"));
}

#[tokio::test]
async fn error_row_a_colleagues_result_cannot_be_kept_beside_mine() {
    let (rig, _, _) = shared().await;
    rig.workspaces
        .write_result(rig.ws, "Pack.md", b"mine", None);
    for n in 1..=99u32 {
        rig.workspaces.write_result(
            rig.ws,
            &files::both_kept_name("Pack.md", "asha-patel", n),
            b"taken",
            None,
        );
    }
    rig.host
        .commit(&[("results/Pack.md", Some(b"theirs"))], "Priya");
    rig.get().await.unwrap();
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.path == "results/Pack.md" && p.code == "TEAM_WS_RESULT_NOT_TAKEN"));
    assert_eq!(rig.workspaces.result(rig.ws, "Pack.md").unwrap(), b"mine");
}

#[tokio::test]
async fn error_row_this_person_cannot_open_the_results_place() {
    let (rig, _, _) = shared().await;
    let mut d = description(&rig.host);
    let key = uuid::Uuid::now_v7().hyphenated().to_string();
    d.places.push(files::TeamPlaceEntry {
        key: key.clone(),
        kind: "sharepoint".into(),
        location: "graph:drives/b!z/items/RESULTS".into(),
        display_name: "Supplier results".into(),
        changed_by: "priya-shah".into(),
        changed_at: ordin_types::time::Timestamp::now(),
    });
    d.places.sort_by(|a, b| a.key.cmp(&b.key));
    d.results = files::TeamResultsEntry {
        kind: "sharepoint".into(),
        place: Some(key),
        changed_by: "priya-shah".into(),
        changed_at: ordin_types::time::Timestamp::now(),
    };
    rig.workspaces.state.lock().unwrap().link_refused.insert(
        "graph:drives/b!z/items/RESULTS".into(),
        ordin_workspaces::WorkspaceError::SourceAccessDenied,
    );
    rig.host.commit(
        &[(files::DESCRIPTION_FILE, Some(&files::render(&d)))],
        "Priya Shah (priya-shah)",
    );
    rig.get().await.unwrap();
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_RESULTS_NO_ACCESS"));
    assert!(rig.status().await.team_workspace.unwrap().results_here_only);
}

// ---- the share scope of a team workspace ----

#[tokio::test]
async fn share_what_weve_learned_refused_while_the_first_share_waits() {
    let (rig, _) = background().await;
    rig.service
        .share_workspace(rig.ws, rig.choice(), TeamResultsChoice::TeamFolder {})
        .await
        .unwrap();
    rig.wait_idle().await;
    assert_eq!(
        rig.service.share(rig.ws).await.unwrap_err().code,
        "TEAM_SHARE_PENDING"
    );
    let _ = ShareScope::All;
}

// ---- amended January 2026 (user decision): Change… after sharing, results to SharePoint and
// Confluence, the GitHub login ----

const RESULTS_SP: &str = "graph:drives/d1/items/f1";
const RESULTS_PAGE: &str = "confluence:page/4001";

fn sharepoint_folder() -> ordin_workspaces::LinkLocation {
    ordin_workspaces::LinkLocation::Sharepoint {
        location: ordin_workspaces::SharePointLocation {
            site_id: "s1".into(),
            site_name: "Suppliers".into(),
            drive_id: "d1".into(),
            drive_name: "Documents".into(),
            item_id: Some("f1".into()),
            path: "Supplier results".into(),
            is_folder: true,
            web_url: "https://acme.sharepoint.com/sites/suppliers/Supplier%20results".into(),
        },
    }
}

fn confluence_page() -> ordin_workspaces::LinkLocation {
    ordin_workspaces::LinkLocation::Confluence {
        location: ordin_workspaces::ConfluenceLocation {
            space_key: "PROC".into(),
            space_name: "Procurement".into(),
            page_id: Some("4001".into()),
            title: "Supplier results".into(),
            web_url: "https://acme.atlassian.net/wiki/spaces/PROC/pages/4001".into(),
        },
    }
}

/// W shared as a team workspace, with its results place moved to a link of `kind` at `origin`
/// whose location is `location`.
async fn results_at(
    kind: SourceLinkKind,
    origin: &str,
    location: ordin_workspaces::LinkLocation,
) -> (Rig, ordin_types::AgentInstanceId) {
    let (rig, analyst, _) = shared().await;
    let link = rig
        .workspaces
        .link(rig.ws, kind, origin, "Supplier results");
    rig.workspaces
        .state
        .lock()
        .unwrap()
        .locations
        .insert(origin.to_owned(), location);
    rig.service
        .set_results_place(
            rig.ws,
            TeamResultsChoice::Link {
                source_link_id: link.id,
            },
        )
        .await
        .expect("set the results place");
    (rig, analyst)
}

async fn run_writing(rig: &Rig, analyst: ordin_types::AgentInstanceId, path: &str) {
    let run = RunId::new();
    rig.workspaces
        .write_result(rig.ws, path, b"# Comparison\n", Some(run));
    rig.runs
        .runs
        .lock()
        .unwrap()
        .insert(run, run_facts(rig.ws, analyst, "Compare documents"));
    rig.service.record_run(run).await;
}

fn unsent(rig: &Rig, path: &str) -> bool {
    !rig.workspaces.state.lock().unwrap().results[&rig.ws][path].sent
}

// Scenario: Happy path — Change… reads where results may be saved after sharing (TW-K)
// spec: ordin-desktop/components/TeamKnowledgeService#TW-K
#[tokio::test]
async fn happy_path_change_reads_where_results_may_be_saved_after_sharing() {
    let (rig, _, _) = shared().await;
    let link = rig.workspaces.link(
        rig.ws,
        SourceLinkKind::SharepointLocation,
        RESULTS_SP,
        "Supplier results",
    );
    // The share preview refuses a team workspace; the results choices answer.
    assert_eq!(
        rig.service
            .workspace_share_preview(rig.ws, rig.choice())
            .await
            .unwrap_err()
            .code,
        "TEAM_WS_ALREADY_TEAM"
    );
    let c = rig.service.results_choices(rig.ws).await.expect("choices");
    let names: Vec<&str> = c.choices.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "acme/supplier-team › knowledge › results",
            "Contracts",
            "Supplier results"
        ]
    );
    assert_eq!(c.choices[0].choice, TeamResultsChoice::TeamFolder {});
    assert_eq!(c.current, Some(TeamResultsChoice::TeamFolder {}));
    let status = rig
        .service
        .set_results_place(
            rig.ws,
            TeamResultsChoice::Link {
                source_link_id: link.id,
            },
        )
        .await
        .expect("set");
    assert_eq!(status.team_workspace.unwrap().results, "Supplier results");
    let c = rig.service.results_choices(rig.ws).await.expect("choices");
    assert_eq!(
        c.current,
        Some(TeamResultsChoice::Link {
            source_link_id: link.id
        })
    );
}

// Scenario: Error — results choices of a workspace that is not a team workspace (TW-K)
#[tokio::test]
async fn error_row_results_choices_of_a_workspace_that_is_not_a_team_workspace() {
    let rig = Rig::new().await;
    rig.on().await;
    assert_eq!(
        rig.service.results_choices(rig.ws).await.unwrap_err().code,
        "TEAM_WS_NOT_TEAM"
    );
    assert_eq!(
        rig.service
            .set_results_place(rig.ws, TeamResultsChoice::TeamFolder {})
            .await
            .unwrap_err()
            .code,
        "TEAM_WS_NOT_TEAM"
    );
}

// Scenario: Happy path — a result goes to the SharePoint results folder (TW-G step 3)
// spec: ordin-desktop/components/TeamKnowledgeService#TW-G
#[tokio::test]
async fn happy_path_a_result_goes_to_the_sharepoint_results_folder() {
    let (rig, analyst) = results_at(
        SourceLinkKind::SharepointLocation,
        RESULTS_SP,
        sharepoint_folder(),
    )
    .await;
    let cards = rig.push.count();
    run_writing(&rig, analyst, "Kestrel quote — comparison.md").await;
    let published = rig.push.published.lock().unwrap().clone();
    assert_eq!(published.len(), 1);
    let r = &published[0];
    assert_eq!(
        r.destination,
        ordin_types::domain::places::CopyDestination::SharepointFile {
            site: "s1".into(),
            library: "d1".into(),
            folder: "f1".into(),
        }
    );
    assert_eq!(r.format, ordin_types::domain::places::CopyFormat::ExactFile);
    assert!(!r.keep_up_to_date);
    assert_eq!(r.file_name, "Kestrel quote — comparison.md");
    let results = rig.workspaces.state.lock().unwrap().results_folder[&rig.ws];
    assert_eq!(
        r.original,
        ordin_types::domain::places::CopyOriginal::PlaceFile {
            place_id: results,
            path: "Kestrel quote — comparison.md".into(),
        }
    );
    assert!(
        !unsent(&rig, "Kestrel quote — comparison.md"),
        "marked sent"
    );
    // No team-folder share is proposed for it.
    assert_eq!(rig.push.count(), cards);
}

// Scenario: Happy path — a result goes to the Confluence results page (TW-G step 3)
// spec: ordin-desktop/components/TeamKnowledgeService#TW-G
#[tokio::test]
async fn happy_path_a_result_goes_to_the_confluence_results_page() {
    let (rig, analyst) = results_at(
        SourceLinkKind::ConfluenceLocation,
        RESULTS_PAGE,
        confluence_page(),
    )
    .await;
    run_writing(&rig, analyst, "Kestrel quote — comparison.md").await;
    let published = rig.push.published.lock().unwrap().clone();
    assert_eq!(published.len(), 1);
    assert_eq!(
        published[0].destination,
        ordin_types::domain::places::CopyDestination::ConfluenceAttachment {
            space: "PROC".into(),
            parent_page_id: "4001".into(),
        }
    );
    assert_eq!(
        published[0].format,
        ordin_types::domain::places::CopyFormat::ExactFile
    );
}

// Scenario: Error — the results place cannot be read (TW-G step 3)
#[tokio::test]
async fn error_row_the_results_place_cannot_be_read() {
    let (rig, analyst) = results_at(
        SourceLinkKind::SharepointLocation,
        RESULTS_SP,
        sharepoint_folder(),
    )
    .await;
    rig.workspaces.state.lock().unwrap().resolve_refused.insert(
        RESULTS_SP.into(),
        ordin_workspaces::WorkspaceError::SourceAccessDenied,
    );
    run_writing(&rig, analyst, "Pack.md").await;
    assert!(
        rig.push.published.lock().unwrap().is_empty(),
        "nothing proposed"
    );
    assert!(unsent(&rig, "Pack.md"));
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_RESULTS_NO_ACCESS"));
    // Readable again: the next run's result goes and the problem is cleared.
    rig.workspaces.state.lock().unwrap().resolve_refused.clear();
    run_writing(&rig, analyst, "Pack 2.md").await;
    assert_eq!(rig.push.published.lock().unwrap().len(), 1);
    assert!(!rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_RESULTS_NO_ACCESS"));
}

// Error Table: the results link no longer names a folder (TW-G step 3).
#[tokio::test]
async fn error_row_the_results_link_no_longer_names_a_folder() {
    let mut file = sharepoint_folder();
    if let ordin_workspaces::LinkLocation::Sharepoint { location } = &mut file {
        location.is_folder = false;
    }
    let (rig, analyst) = results_at(SourceLinkKind::SharepointLocation, RESULTS_SP, file).await;
    run_writing(&rig, analyst, "Pack.md").await;
    assert!(rig.push.published.lock().unwrap().is_empty());
    assert!(unsent(&rig, "Pack.md"));
    assert!(rig
        .status()
        .await
        .problems
        .iter()
        .any(|p| p.code == "TEAM_WS_RESULTS_NOT_SUITABLE"));
}

// TW-G step 3: a refused publish leaves the result unsent with its code as a problem.
#[tokio::test]
async fn a_refused_publish_leaves_the_result_unsent() {
    let (rig, analyst) = results_at(
        SourceLinkKind::SharepointLocation,
        RESULTS_SP,
        sharepoint_folder(),
    )
    .await;
    *rig.push.refuse_publish.lock().unwrap() = Some(ordin_push::PushError::new(
        "PUSH_DESTINATION_REFUSED",
        "refused",
    ));
    run_writing(&rig, analyst, "Pack.md").await;
    assert!(unsent(&rig, "Pack.md"));
    let problems = rig.status().await.problems;
    assert!(
        problems
            .iter()
            .any(|p| p.path == "results/Pack.md" && p.code == "PUSH_DESTINATION_REFUSED"),
        "{problems:?}"
    );
}

// Scenario: Happy path — a person's name is their GitHub login (SA-TEAM-TW-02)
// spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-TW-02
#[tokio::test]
async fn happy_path_a_persons_name_is_their_github_login() {
    let (rig, _) = background().await;
    crate::service::lock(&rig.service.seams).login = None;
    rig.repos
        .person
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .github_login = Some("asha-gh".into());
    rig.service
        .share_workspace(rig.ws, rig.choice(), TeamResultsChoice::TeamFolder {})
        .await
        .expect("share");
    rig.wait_idle().await;
    let p = rig.push.last();
    rig.approve(&p).await;
    let d = description(&rig.host);
    assert_eq!(d.name_changed_by, "asha-gh");
    assert!(d.places.iter().all(|x| x.changed_by == "asha-gh"));
    assert_eq!(d.results.changed_by, "asha-gh");
}

// Scenario: Error — no GitHub login and no named sign-in (SA-TEAM-TW-02)
#[tokio::test]
async fn error_no_github_login_and_no_named_sign_in() {
    let (rig, _) = background().await;
    crate::service::lock(&rig.service.seams).login = None;
    rig.repos
        .person
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .github_login = None;
    assert_eq!(
        rig.service
            .workspace_share_preview(rig.ws, rig.choice())
            .await
            .unwrap_err()
            .code,
        "TEAM_SIGN_IN_NEEDED"
    );
    // A GitHub login that breaks LOGIN_PATTERN is no login either.
    rig.repos
        .person
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .github_login = Some("-bad login".into());
    assert_eq!(
        rig.service
            .workspace_share_preview(rig.ws, rig.choice())
            .await
            .unwrap_err()
            .code,
        "TEAM_SIGN_IN_NEEDED"
    );
}
