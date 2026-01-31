//! X2: one test per scenario of PackageCatalogue.md *Acceptance Criteria (Gherkin)*, named after it.

use std::sync::Arc;

use ordin_catalogue::{LibraryEntry, PartStateLabel};
use ordin_checker::{InstallVerdict, TrustChanges};
use ordin_installers::VerifyOutcome;
use ordin_package_inspector::MayDoChangeKind;
use ordin_resolver::model::{ChosenBy, PartKind, SelectedPart};
use ordin_resolver::ResolveError;
use ordin_types::domain::agents::{AgentInstanceStatus, StoppedBy, WithdrawnListEntry};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, WithdrawnListEntryId};

use super::support::*;
use crate::types::{
    AddOutcome, AgentMatch, AgentNotice, AgentRepositoryRef, Availability, ChangeChoiceInput,
    PackChoice, RemoveAgentInput,
};

fn withdrawn_entry(
    repo: ordin_types::AgentRepositoryId,
    replacement: Option<&str>,
) -> WithdrawnListEntry {
    WithdrawnListEntry {
        id: WithdrawnListEntryId::new(),
        agent_repository_id: repo,
        part_kind: PartKind::PracticePack,
        part_identifier: GO_PACK.to_owned(),
        part_version: None,
        part_content_digest: None,
        reason: "unsafe".to_owned(),
        withdrawn_at: Timestamp::now(),
        replacement_identifier: replacement.map(str::to_owned),
        replacement_version: None,
        removed_by_hand_seen_at: None,
        read_at: Timestamp::now(),
    }
}

/// A plan that asks the testing question until it is answered, then uses the chosen pack.
fn asking_plan() -> ordin_resolver::model::ResolutionPlan {
    let mut asking = plan("a1", COMMIT_A, "1.2.0");
    asking.selections.pending_choices = vec![pending(0, "testing", &[PYTEST_PACK, UNITTEST_PACK])];
    asking
}

fn answering_resolver(fx: &Fx) {
    let asking = asking_plan();
    let chosen = |chosen_by: ChosenBy| {
        let mut p = plan("a2", COMMIT_A, "1.2.0");
        p.selections.practice_packs.push(SelectedPart {
            id: PYTEST_PACK.to_owned(),
            chosen_by,
        });
        p.selections.pending_choices.clear();
        p
    };
    let (by_answer, by_change) = (
        chosen(ChosenBy::PersonAnswer),
        chosen(ChosenBy::PersonChange),
    );
    *lock(&fx.resolver.plan_fn) = Some(Arc::new(move |request| {
        if !request.pack_answers.is_empty() {
            Ok(by_answer.clone())
        } else if !request.pack_changes.is_empty() {
            Ok(by_change.clone())
        } else {
            Ok(asking.clone())
        }
    }));
}

#[tokio::test]
async fn happy_path_add_a_clean_trusted_agent() {
    let fx = Fx::new().await;
    let mut notices = fx.pc.subscribe();
    let agent = fx.add().await;
    assert_eq!(agent.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(agent.instance.display_name, "Analyst");
    assert_eq!(agent.instance.package_name, "analyst");
    assert_eq!(
        agent.instance.audit_result,
        Some(ordin_types::domain::agents::AuditResult::Passed)
    );
    assert_eq!(agent.workspaces, vec![fx.workspace]);
    assert_eq!(agent.tasks, vec!["Plan the release".to_owned()]);
    // Its uses list Go, picked from the workspace.
    assert_eq!(agent.uses.len(), 1);
    assert_eq!(agent.uses[0].label, "Go");
    assert_eq!(agent.uses[0].chosen_by, ChosenBy::WorkspaceCode);
    assert!(agent.uses[0].changeable);
    // The workspace's languages reached the Resolver.
    assert_eq!(
        lock(&fx.resolver.requests)[0].workspace_languages,
        vec!["go".to_owned()]
    );
    // The journal records the install with its lock file and check results.
    let entries = fx
        .journal
        .list_entries(
            ordin_journal::EntryFilter {
                run_id: None,
                kind: Some("agent_added".to_owned()),
            },
            None,
        )
        .await
        .expect("entries");
    assert_eq!(entries.items.len(), 1);
    let data = &entries.items[0].data;
    assert_eq!(data["lock_file_sha256"], "1".repeat(64));
    assert!(data["check_run_id"].as_str().is_some_and(|s| s.len() == 36));
    assert_eq!(entries.items[0].component, "package_catalogue");
    // The order of the steps: check, folder, build, install, inspect, decide, workspaces, prune.
    let log = fx.log_lines();
    let at = |what: &str| log.iter().position(|l| l == what).expect(what);
    assert!(at("checker.check_for_install") < at("folders.create"));
    assert!(at("folders.create") < at("installers.install"));
    assert!(at("installers.install") < at("inspector.inspect"));
    assert!(at("inspector.inspect") < at("checker.decide_executable_parts"));
    assert!(at("checker.decide_executable_parts") < at("inspector.set_workspaces"));
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::Added { .. }
    ));
    let kept = lock(&fx.resolver.pruned).last().cloned().expect("a prune");
    assert!(kept.contains(&plan("a1", COMMIT_A, "1.2.0").plan_hash));
}

#[tokio::test]
async fn agent_needs_it_approval() {
    let fx = Fx::new().await;
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("hook")],
    };
    let outcome = fx.pc.add(fx.input()).await.expect("an outcome");
    assert!(
        matches!(outcome, AddOutcome::NeedsItApproval { ref parts } if parts == &["hook".to_owned()])
    );
    assert_eq!(fx.instance_count().await, 0, "no instance");
    assert_eq!(count(&fx.log_lines(), "folders.create"), 0, "no folder");
}

#[tokio::test]
async fn agent_fails_its_checks() {
    let fx = Fx::new().await;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    let outcome = fx.pc.add(fx.input()).await.expect("an outcome");
    assert!(matches!(outcome, AddOutcome::Unsafe));
    assert_eq!(fx.instance_count().await, 0);
    assert_eq!(count(&fx.log_lines(), "folders.create"), 0);
}

#[tokio::test]
async fn required_pack_the_workspace_cannot_settle() {
    let fx = Fx::new().await;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::RequiredPackUnsettled {
        rule_index: 0,
        class: Some("testing".to_owned()),
    });
    let mut input = fx.input();
    input.workspace_languages.clear();
    let preview = fx.pc.preview_add(input).await.expect("a preview");
    assert_eq!(
        preview.available,
        Availability::PackMissing {
            pack_class_label: "testing".to_owned()
        }
    );
}

#[tokio::test]
async fn interrupted_install_retried_once() {
    let fx = Fx::new().await;
    fx.installers
        .install_failures
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let agent = fx.add().await;
    assert_eq!(agent.instance.status, AgentInstanceStatus::Ready);
    let log = fx.log_lines();
    assert_eq!(count(&log, "folders.create"), 2);
    assert_eq!(
        count(&log, "folders.remove"),
        1,
        "the first folder is removed"
    );
    assert_eq!(count(&log, "installers.uninstall"), 1);
    assert_eq!(count(&log, "installers.install"), 2);
}

#[tokio::test]
async fn silent_update_between_runs() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let mut notices = fx.pc.subscribe();
    fx.activity.start(id);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    // The candidate has the same tools, connections and reads as the installed agent.
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let waiting = fx.pc.get(id).await.expect("the instance");
    assert_eq!(waiting.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(
        waiting.instance.package_version, "1.2.0",
        "nothing changes until the run ends"
    );
    assert_eq!(count(&fx.log_lines(), "installers.update"), 0);
    fx.activity.finish(id);
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.updated_between_runs_at.is_some())
    })
    .await;
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(updated.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(updated.instance.package_version, "1.3.0");
    assert_eq!(updated.instance.commit.as_deref(), Some(COMMIT_B));
    assert!(updated.update.is_none());
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::UpdatedBetweenRuns { .. }
    ));
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
}

#[tokio::test]
async fn update_that_changes_what_the_agent_may_do() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let mut notices = fx.pc.subscribe();
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let waiting = fx.pc.get(id).await.expect("the instance");
    assert_eq!(
        waiting.instance.status,
        AgentInstanceStatus::UpdateAwaitingChoice
    );
    let update = waiting.update.expect("a pending update");
    assert_eq!(update.to_commit, COMMIT_B);
    assert_eq!(update.may_do_changes.len(), 1);
    assert_eq!(
        update.may_do_changes[0].kind,
        MayDoChangeKind::ConnectionAdded
    );
    assert_eq!(update.may_do_changes[0].name, "mail");
    assert_eq!(
        update.may_do_changes[0].message_id,
        "may-do-connection-added"
    );
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::UpdateNeedsChoice { .. }
    ));
    assert_eq!(
        count(&fx.log_lines(), "installers.update"),
        0,
        "nothing is installed yet"
    );
}

#[tokio::test]
async fn withdrawn_part_with_no_replacement() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let mut notices = fx.pc.subscribe();
    let entry = withdrawn_entry(fx.repo, None);
    lock(&fx.catalogue.withdrawn).push(entry.clone());
    *lock(&fx.resolver.build_error) = Some(ResolveError::PartWithdrawn {
        part: GO_PACK.to_owned(),
        replacement: None,
    });
    fx.pc
        .on_repository_updated(fx.repo, &[entry.id], &shutdown())
        .await;
    let stopped = fx.pc.get(id).await.expect("the instance");
    assert_eq!(stopped.instance.status, AgentInstanceStatus::Refused);
    assert_eq!(stopped.instance.stopped_by, Some(StoppedBy::Withdrawal));
    assert_eq!(
        stopped.instance.stopped_reason.as_deref(),
        Some("Analyst stopped: go was withdrawn by Acme agents.")
    );
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::Stopped { .. }
    ));
    assert_eq!(fx.journal_kinds("agent_stopped").await, 1);
}

#[tokio::test]
async fn the_agent_asks_which_pack_to_use() {
    let fx = Fx::new().await;
    fx.set_plan(asking_plan());
    let agent = fx.add().await;
    let id = agent.instance.id;
    answering_resolver(&fx);
    fx.clear_log();
    let answered = fx
        .pc
        .record_answer(ChangeChoiceInput {
            agent_instance_id: id,
            choice: PackChoice {
                rule_index: 0,
                packs: vec![PYTEST_PACK.to_owned()],
            },
        })
        .await
        .expect("answered");
    assert_eq!(answered.instance.status, AgentInstanceStatus::Ready);
    // The choice is stored with source agent_asked.
    let stored = sqlx::query_scalar::<_, String>(
        "SELECT source FROM agent_instance_choices WHERE rule_index = 0",
    )
    .fetch_one(&fx.pool)
    .await
    .expect("the stored choice");
    assert_eq!(stored, "agent_asked");
    {
        let requests = lock(&fx.resolver.requests);
        let last = requests.last().expect("a request");
        assert_eq!(last.pack_answers.len(), 1);
        assert_eq!(last.pack_answers[0].pack_ids, vec![PYTEST_PACK.to_owned()]);
    }
    // The bundle is rebuilt and the folder updated, in the session's next turn, before the agent goes on.
    let log = fx.log_lines();
    assert!(log.contains(&"resolver.build".to_owned()));
    assert_eq!(count(&log, "installers.update_in_session"), 1);
    assert_eq!(count(&log, "installers.update"), 0);
}

#[tokio::test]
async fn verify_agent_folders_without_repair() {
    let fx = Fx::new().await;
    let first = fx.add().await;
    let other = fx.second_agent("urn:ordin:acme:agents:planner", "Release planner");
    let second = fx.add_agent(other).await;
    lock(&fx.installers.verify_for).insert(
        second.instance.id,
        VerifyOutcome::Mismatch {
            changed: vec!["AGENTS.md".to_owned()],
            missing: Vec::new(),
        },
    );
    let _ = first;
    fx.clear_log();
    let report = fx.pc.verify_agent_folders(false).await.expect("a report");
    assert_eq!(report.checked, 2);
    assert_eq!(report.repaired, 0);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].code, "PKG_FOLDER_MISMATCH");
    assert_eq!(report.failed[0].agent_name, "Release planner");
    let log = fx.log_lines();
    assert_eq!(
        count(&log, "installers.verify_and_repair"),
        0,
        "no folder is changed"
    );
}

#[tokio::test]
async fn verify_agent_folders_with_repair() {
    let fx = Fx::new().await;
    fx.add().await;
    *lock(&fx.installers.repair) = Ok(true);
    let report = fx.pc.verify_agent_folders(true).await.expect("a report");
    assert_eq!(report.checked, 1);
    assert_eq!(report.repaired, 1);
    assert!(report.failed.is_empty());
    assert_eq!(fx.journal_kinds("agent_repaired").await, 1);
}

#[tokio::test]
async fn workflow_agent_at_another_version() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let key = AgentRepositoryRef::ProfileKey("acme-agents".to_owned());
    let found = fx
        .pc
        .match_workflow_agent(fx.workspace, &key, AGENT_IDENTIFIER, "1.3.0")
        .await
        .expect("a match");
    assert_eq!(
        found,
        AgentMatch::OtherVersion {
            agent_instance_id: agent.instance.id,
            display_name: "Analyst".to_owned(),
            installed_version: "1.2.0".to_owned()
        }
    );
    let same = fx
        .pc
        .match_workflow_agent(fx.workspace, &key, AGENT_IDENTIFIER, "1.2.0")
        .await
        .expect("a match");
    assert!(matches!(same, AgentMatch::Present { .. }));
    let none = fx
        .pc
        .match_workflow_agent(fx.workspace, &key, "urn:ordin:acme:agents:other", "1.2.0")
        .await
        .expect("a match");
    assert_eq!(none, AgentMatch::Missing);
    // A repository the library does not know.
    *lock(&fx.catalogue.by_ref) = Ok(None);
    let unknown = fx
        .pc
        .match_workflow_agent(fx.workspace, &key, AGENT_IDENTIFIER, "1.2.0")
        .await
        .expect("a match");
    assert_eq!(unknown, AgentMatch::Missing);
}

#[tokio::test]
async fn find_agents_for_a_need() {
    let fx = Fx::new().await;
    let rate = fx.second_agent("urn:ordin:acme:agents:rates", "Rate comparison");
    let rate_view = lock(&fx.catalogue.parts)
        .iter()
        .find(|v| v.part.id == rate)
        .cloned()
        .expect("the view");
    lock(&fx.catalogue.entries).push(LibraryEntry {
        part: rate_view,
        from: "Acme agents".to_owned(),
    });
    let found = fx
        .pc
        .find_agents(fx.workspace, "  compare supplier day rates ")
        .await
        .expect("agents");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Rate comparison");
    assert_eq!(found[0].identifier, "urn:ordin:acme:agents:rates");
    assert_eq!(found[0].summary, "Compares things.");
    assert_eq!(found[0].from, "Acme agents");
    assert_eq!(found[0].agent, rate);
    assert_eq!(found[0].in_workspace, None);
    assert!(fx
        .log_lines()
        .contains(&"search:compare supplier day rates".to_owned()));
    // Once added, the entry is marked.
    fx.add_agent(rate).await;
    let again = fx
        .pc
        .find_agents(fx.workspace, "compare")
        .await
        .expect("agents");
    assert!(again[0].in_workspace.is_some());
}

#[tokio::test]
async fn proposal_of_a_withdrawn_agent() {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts)[0].state_label = PartStateLabel::Withdrawn;
    let error = fx
        .pc
        .check_proposal(fx.repo, AGENT_IDENTIFIER)
        .await
        .unwrap_err();
    assert_eq!(error.code, "PKG_AGENT_NOT_OFFERED");
}

#[tokio::test]
async fn agent_that_may_send_email() {
    let fx = Fx::new().await;
    *lock(&fx.inspector.may_do) = may_do(&["files", "mail"]);
    let agent = fx.add().await;
    assert!(fx.pc.can_send_email(agent.instance.id).await);
    assert!(!fx.pc.can_send_email(AgentInstanceId::new()).await);
}

#[tokio::test]
async fn remove_after_confirmation() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    fx.clear_log();
    fx.pc
        .remove(RemoveAgentInput {
            agent_instance_id: agent.instance.id,
            workspace_id: None,
        })
        .await
        .expect("removed");
    let log = fx.log_lines();
    let at = |what: &str| log.iter().position(|l| l == what).expect(what);
    assert!(at("installers.uninstall") < at("inspector.forget"));
    assert!(at("inspector.forget") < at("checker.forget_instance"));
    assert!(at("checker.forget_instance") < at("folders.remove"));
    let after = fx.pc.get(agent.instance.id).await.expect("still readable");
    assert_eq!(after.instance.status, AgentInstanceStatus::Removed);
    // The Resolver prunes bundles no longer named by any instance.
    let kept = lock(&fx.resolver.pruned).last().cloned().expect("a prune");
    assert!(kept.is_empty());
    assert_eq!(fx.journal_kinds("agent_removed").await, 1);
}

#[tokio::test]
async fn agents_of_a_workspace_for_the_knowledge_service() {
    let fx = Fx::new().await;
    fx.add().await;
    let planner = fx.second_agent("urn:ordin:acme:agents:planner", "Release planner");
    let old = fx.second_agent("urn:ordin:acme:agents:old", "Old helper");
    fx.add_agent(planner).await;
    let old_agent = fx.add_agent(old).await;
    fx.pc
        .remove(RemoveAgentInput {
            agent_instance_id: old_agent.instance.id,
            workspace_id: None,
        })
        .await
        .expect("removed");
    let agents = fx
        .pc
        .agents_in_workspace(fx.workspace)
        .await
        .expect("agents");
    let names: Vec<&str> = agents.iter().map(|a| a.display_name.as_str()).collect();
    assert_eq!(names, vec!["Analyst", "Release planner"]);
    assert!(fx
        .pc
        .agents_in_workspace(ordin_types::WorkspaceId::new())
        .await
        .expect("an unknown workspace is not an error")
        .is_empty());
}

#[tokio::test]
async fn trust_lost_when_the_company_s_profile_changes() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let mut notices = fx.pc.subscribe();
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: vec![id],
            gained: Vec::new(),
        })
        .await
        .expect("applied");
    let stopped = fx.pc.get(id).await.expect("the instance");
    assert_eq!(stopped.instance.status, AgentInstanceStatus::Refused);
    assert_eq!(stopped.instance.stopped_by, Some(StoppedBy::Trust));
    assert_eq!(
        stopped.instance.stopped_reason.as_deref(),
        Some("Analyst stopped: your company's rules no longer allow one of the programs it uses. Your other agents still work.")
    );
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::Stopped { .. }
    ));
    let error = fx.pc.ensure_ready(id).await.unwrap_err();
    assert_eq!(error.code, "PKG_INSTANCE_NOT_READY");
}

#[tokio::test]
async fn add_stores_the_complete_executable_part_decisions() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    {
        let checks = lock(&fx.checker.checks);
        assert_eq!(
            checks[0].agent_instance_id,
            Some(agent.instance.id),
            "the check runs with the new instance's id"
        );
    }
    {
        let decided = lock(&fx.checker.decided);
        assert_eq!(decided.len(), 1);
        assert_eq!(decided[0].0, agent.instance.id);
        let names: Vec<&str> = decided[0].1.iter().map(|p| p.part_name.as_str()).collect();
        assert_eq!(names, vec!["files", "jira-reader"]);
    }
    // A preview stores nothing: it passes no instance id.
    let fx = Fx::new().await;
    fx.pc.preview_add(fx.input()).await.expect("a preview");
    assert_eq!(lock(&fx.checker.checks)[0].agent_instance_id, None);
}

#[tokio::test]
async fn trust_regained_after_it_approves() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: vec![id],
            gained: Vec::new(),
        })
        .await
        .expect("applied");
    let mut notices = fx.pc.subscribe();
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: Vec::new(),
            gained: vec![id],
        })
        .await
        .expect("applied");
    let back = fx.pc.get(id).await.expect("the instance");
    assert_eq!(back.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(back.instance.stopped_reason, None);
    assert_eq!(back.instance.stopped_by, None);
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::Resumed { .. }
    ));
    assert_eq!(fx.journal_kinds("agent_resumed").await, 1);
}

#[tokio::test]
async fn a_fixed_version_brings_a_withdrawn_agent_back() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let entry = withdrawn_entry(fx.repo, None);
    lock(&fx.catalogue.withdrawn).push(entry.clone());
    *lock(&fx.resolver.build_error) = Some(ResolveError::PartWithdrawn {
        part: GO_PACK.to_owned(),
        replacement: None,
    });
    fx.pc
        .on_repository_updated(fx.repo, &[entry.id], &shutdown())
        .await;
    assert_eq!(
        fx.pc.get(id).await.expect("the instance").instance.status,
        AgentInstanceStatus::Refused
    );
    // A new commit whose agent no longer uses that part, and passes its checks.
    *lock(&fx.resolver.build_error) = None;
    fx.set_plan(plan("c1", COMMIT_B, "1.3.0"));
    let mut notices = fx.pc.subscribe();
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let waiting = fx.pc.get(id).await.expect("the instance");
    assert_eq!(
        waiting.instance.status,
        AgentInstanceStatus::Refused,
        "the status stays refused"
    );
    assert_eq!(waiting.update.expect("an update waits").to_commit, COMMIT_B);
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::UpdateNeedsChoice { .. }
    ));
    // The person accepts the update.
    let accepted = fx.pc.accept_update(id).await.expect("accepted");
    assert_eq!(accepted.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(accepted.instance.stopped_reason, None);
    assert_eq!(accepted.instance.package_version, "1.3.0");
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::Resumed { .. }
    ));
    assert_eq!(fx.journal_kinds("agent_resumed").await, 1);
}

#[tokio::test]
async fn source_scope_of_a_trial() {
    let fx = Fx::new().await;
    let trial = fx.pc.add_trial(fx.trial_input()).await.expect("a trial");
    let error = fx.pc.source_scope(trial.instance.id).await.unwrap_err();
    assert_eq!(error.code, "PKG_TRIAL_NO_SCOPE");
    // An installed library agent has its home repository's scope.
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let scope = fx
        .pc
        .source_scope(agent.instance.id)
        .await
        .expect("a scope");
    assert_eq!(
        scope.home,
        ordin_resolver::model::RepositoryRef::Repository(fx.repo)
    );
}

#[tokio::test]
async fn open_pack_question_of_an_installed_agent() {
    let fx = Fx::new().await;
    fx.set_plan(asking_plan());
    let agent = fx.add().await;
    let id = agent.instance.id;
    answering_resolver(&fx);
    let open = fx.pc.pending_choices(id).await.expect("the open questions");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].agent_instance_id, id);
    assert_eq!(open[0].rule_index, 0);
    assert_eq!(open[0].at_least, 1);
    assert_eq!(open[0].class_label, "testing");
    let options: Vec<(&str, &str)> = open[0]
        .choices
        .iter()
        .map(|c| (c.pack.as_str(), c.label.as_str()))
        .collect();
    assert_eq!(
        options,
        vec![(PYTEST_PACK, "pytest"), (UNITTEST_PACK, "unittest")]
    );
    fx.pc
        .record_answer(ChangeChoiceInput {
            agent_instance_id: id,
            choice: PackChoice {
                rule_index: 0,
                packs: vec![PYTEST_PACK.to_owned()],
            },
        })
        .await
        .expect("answered");
    let stored = sqlx::query_scalar::<_, String>(
        "SELECT source FROM agent_instance_choices WHERE rule_index = 0",
    )
    .fetch_one(&fx.pool)
    .await
    .expect("the stored choice");
    assert_eq!(stored, "agent_asked");
    assert!(fx
        .pc
        .pending_choices(id)
        .await
        .expect("none open")
        .is_empty());
}

#[tokio::test]
async fn pack_change_while_the_agent_is_working() {
    let fx = Fx::new().await;
    fx.set_plan(asking_plan());
    let agent = fx.add().await;
    let id = agent.instance.id;
    answering_resolver(&fx);
    fx.activity.start(id);
    fx.clear_log();
    let changed = fx
        .pc
        .change_choice(ChangeChoiceInput {
            agent_instance_id: id,
            choice: PackChoice {
                rule_index: 0,
                packs: vec![PYTEST_PACK.to_owned()],
            },
        })
        .await
        .expect("changed");
    assert_eq!(changed.instance.status, AgentInstanceStatus::Ready);
    let log = fx.log_lines();
    assert_eq!(count(&log, "installers.update_in_session"), 1);
    assert_eq!(count(&log, "installers.update"), 0);
    let stored = sqlx::query_scalar::<_, String>(
        "SELECT source FROM agent_instance_choices WHERE rule_index = 0",
    )
    .fetch_one(&fx.pool)
    .await
    .expect("the stored choice");
    assert_eq!(stored, "person_change");
    {
        let requests = lock(&fx.resolver.requests);
        assert_eq!(
            requests.last().expect("a request").pack_changes,
            vec![PYTEST_PACK.to_owned()]
        );
    }
    // With no running session the change is installed with `update`.
    fx.activity.finish(id);
    fx.clear_log();
    fx.pc
        .change_choice(ChangeChoiceInput {
            agent_instance_id: id,
            choice: PackChoice {
                rule_index: 0,
                packs: vec![PYTEST_PACK.to_owned()],
            },
        })
        .await
        .expect("changed");
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
}

// ---- Amendment January 2026 — an installed agent's task forms (ADR-119) ----

fn task_entry(
    command: &str,
    name: &str,
    title: Option<&str>,
    hint: Option<&str>,
) -> ordin_package_inspector::TaskEntry {
    ordin_package_inspector::TaskEntry {
        command_id: format!("urn:ordin:default:commands:documents:{command}"),
        name: name.to_owned(),
        description: format!("{name}, in one sentence."),
        session_name: None,
        argument_hint: hint.map(str::to_owned),
        title: title.map(str::to_owned),
        inputs: title.map(|_| {
            vec![ordin_package_inspector::TaskInput {
                name: "document".into(),
                label: "Document".into(),
                kind: ordin_package_inspector::TaskInputKind::Document,
                required: true,
                hint: None,
                choices: Vec::new(),
                max: None,
                default: None,
                whole: false,
            }]
        }),
        form_unreadable: false,
    }
}

// Scenario: Happy path — titles from the commands
// spec: ordin-desktop/components/PackageCatalogue#PKG-TF-1
#[tokio::test]
async fn happy_path_titles_from_the_commands() {
    let fx = Fx::new().await;
    let titles = [
        ("summarise-document", "Summarise a document"),
        ("compare-documents", "Compare documents"),
        ("draft-document", "Draft a document"),
        (
            "rewrite-as-a-person",
            "Rewrite so it reads as a person wrote it",
        ),
    ];
    *lock(&fx.inspector.tasks) = Some(
        titles
            .iter()
            .map(|(slug, title)| task_entry(slug, &format!("/{slug}"), Some(title), None))
            .collect(),
    );
    fx.add().await;
    let listed = fx.pc.list(Some(fx.workspace)).await.expect("agents.list");
    let forms = &listed[0].task_forms;
    let got: Vec<&str> = forms.iter().map(|f| f.title.as_str()).collect();
    assert_eq!(got, titles.iter().map(|(_, t)| *t).collect::<Vec<_>>());
    assert!(forms.iter().all(|f| f.startable));
    assert_eq!(
        listed[0].tasks.len(),
        4,
        "tasks is unchanged beside task_forms"
    );
}

// Scenario: Edge — a task without title or inputs
#[tokio::test]
async fn edge_a_task_without_title_or_inputs() {
    let fx = Fx::new().await;
    *lock(&fx.inspector.tasks) = Some(vec![task_entry(
        "compare-quotes",
        "/compare-quotes",
        None,
        Some("<suppliers and quarter>"),
    )]);
    fx.add().await;
    let listed = fx.pc.list(Some(fx.workspace)).await.expect("agents.list");
    let form = &listed[0].task_forms[0];
    assert_eq!(form.title, "Compare quotes");
    assert_eq!(form.inputs, None);
    assert_eq!(
        form.argument_label.as_deref(),
        Some("suppliers and quarter")
    );
    assert!(form.startable);
}

// PKG-TF-1: an unreadable form is listed and not startable; a model that cannot be read leaves
// task_forms empty, as it leaves tasks empty.
#[tokio::test]
async fn edge_an_unreadable_form_and_an_unreadable_model() {
    let fx = Fx::new().await;
    let mut broken = task_entry("compare-quotes", "/sync_check", None, None);
    broken.form_unreadable = true;
    *lock(&fx.inspector.tasks) = Some(vec![broken]);
    fx.add().await;
    let listed = fx.pc.list(Some(fx.workspace)).await.expect("agents.list");
    assert_eq!(listed[0].task_forms[0].title, "Sync check");
    assert!(!listed[0].task_forms[0].startable);
    assert_eq!(listed[0].task_forms[0].argument_label, None);
    *lock(&fx.inspector.model_error) = Some(ordin_package_inspector::InspectError::NotInspected);
    let listed = fx.pc.list(Some(fx.workspace)).await.expect("agents.list");
    assert!(listed[0].task_forms.is_empty());
    assert!(listed[0].tasks.is_empty());
}
