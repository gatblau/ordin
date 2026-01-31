//! Behaviour the Error Table and the scenarios do not name: the preview, the trial, the update
//! paths, the loop, the record rules and the readers.

use std::sync::Arc;

use ordin_catalogue::CatalogueNotice;
use ordin_checker::InstallVerdict;
use ordin_installers::InstallError;
use ordin_package_inspector::{InspectError, MayDoChange, MayDoChangeKind};
use ordin_resolver::model::{PartKind, RepositoryRef};
use ordin_resolver::ResolveError;
use ordin_types::domain::agents::{AgentInstanceStatus, StoppedBy, WithdrawnListEntry};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, WithdrawnListEntryId, WorkspaceId};

use super::support::*;
use crate::store::{ChoiceSource, InstanceRow, StoredChoice};
use crate::types::{
    AddOutcome, AgentNotice, Availability, ChangeChoiceInput, PackChoice, RemoveAgentInput,
};

fn entry(fx: &Fx, replacement: Option<&str>) -> WithdrawnListEntry {
    WithdrawnListEntry {
        id: WithdrawnListEntryId::new(),
        agent_repository_id: fx.repo,
        part_kind: PartKind::PracticePack,
        part_identifier: GO_PACK.to_owned(),
        part_version: Some("1.0.0".to_owned()),
        part_content_digest: None,
        reason: "unsafe".to_owned(),
        withdrawn_at: Timestamp::now(),
        replacement_identifier: replacement.map(str::to_owned),
        replacement_version: None,
        removed_by_hand_seen_at: None,
        read_at: Timestamp::now(),
    }
}

#[tokio::test]
async fn preview_of_a_clean_agent_lists_what_it_uses_and_asks() {
    let fx = Fx::new().await;
    let mut asking = plan("a1", COMMIT_A, "1.2.0");
    asking.selections.pending_choices = vec![pending(0, "testing", &[PYTEST_PACK])];
    fx.set_plan(asking);
    let preview = fx.pc.preview_add(fx.input()).await.expect("a preview");
    assert_eq!(preview.agent_name, "Analyst");
    assert_eq!(preview.from, "Acme agents");
    assert_eq!(preview.available, Availability::Available);
    assert_eq!(preview.uses.len(), 1);
    assert_eq!(preview.uses[0].label, "Go");
    assert_eq!(preview.asks.len(), 1);
    assert_eq!(fx.instance_count().await, 0, "a preview creates nothing");
    assert_eq!(count(&fx.log_lines(), "folders.create"), 0);
}

#[tokio::test]
async fn preview_of_an_unsafe_agent_and_one_that_needs_it_approval() {
    let fx = Fx::new().await;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    assert_eq!(
        fx.pc
            .preview_add(fx.input())
            .await
            .expect("preview")
            .available,
        Availability::Unsafe
    );
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("files")],
    };
    assert_eq!(
        fx.pc
            .preview_add(fx.input())
            .await
            .expect("preview")
            .available,
        Availability::NeedsItApproval {
            parts: vec!["files".to_owned()]
        }
    );
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::LoopMissing);
    assert_eq!(
        fx.pc
            .preview_add(fx.input())
            .await
            .expect("preview")
            .available,
        Availability::Unsafe
    );
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::SourceUnavailable { retryable: true });
    assert_eq!(
        fx.pc.preview_add(fx.input()).await.unwrap_err().code,
        "PKG_CHECK_UNAVAILABLE"
    );
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::PartWithdrawn {
        part: GO_PACK.to_owned(),
        replacement: None,
    });
    assert_eq!(
        fx.pc.preview_add(fx.input()).await.unwrap_err().code,
        "PKG_AGENT_NOT_OFFERED"
    );
}

#[tokio::test]
async fn a_permanent_resolution_error_is_unsafe_and_journalled() {
    let fx = Fx::new().await;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::LoopMissing);
    let outcome = fx.pc.add(fx.input()).await.expect("an outcome");
    assert!(matches!(outcome, AddOutcome::Unsafe));
    assert_eq!(fx.journal_kinds("agent_add_refused").await, 1);
}

#[tokio::test]
async fn library_failures_while_adding_are_library_unavailable() {
    let fx = Fx::new().await;
    *lock(&fx.catalogue.part_error) = Some(("CAT_STORE_FAILED", true));
    let error = fx.pc.add(fx.input()).await.unwrap_err();
    assert_eq!(
        (error.code, error.retryable),
        ("PKG_LIBRARY_UNAVAILABLE", true)
    );
    *lock(&fx.catalogue.part_error) = None;
    *lock(&fx.catalogue.scope_error) = Some(("CAT_REPOSITORY_UNKNOWN", false));
    let error = fx.pc.add(fx.input()).await.unwrap_err();
    assert_eq!(
        (error.code, error.retryable),
        ("PKG_LIBRARY_UNAVAILABLE", false)
    );
}

#[tokio::test]
async fn an_agent_of_the_bundled_catalogue_is_scoped_to_the_bundle() {
    let fx = Fx::new().await;
    for view in lock(&fx.catalogue.parts).iter_mut() {
        view.part.agent_repository_id = ordin_resolver::model::BUNDLED_REPOSITORY_ID;
    }
    lock(&fx.catalogue.parts)[0].part.commit = "ordin-0.1.0".to_owned();
    let mut bundled = plan("a1", "ordin-0.1.0", "1.2.0");
    for part in bundled
        .resolved
        .iter_mut()
        .chain(std::iter::once(&mut bundled.root))
    {
        part.repository = "bundled".to_owned();
    }
    fx.set_plan(bundled);
    let agent = fx.add().await;
    let scope = fx
        .pc
        .source_scope(agent.instance.id)
        .await
        .expect("a scope");
    assert_eq!(scope.home, RepositoryRef::Bundled);
    assert_eq!(agent.instance.commit.as_deref(), Some("ordin-0.1.0"));
}

#[tokio::test]
async fn a_failing_inspection_step_is_reported_after_two_attempts() {
    let fx = Fx::new().await;
    *lock(&fx.checker.parts_error) = Some(ordin_checker::CheckError::InstanceUnknown {
        id: AgentInstanceId::new(),
    });
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
    let fx = Fx::new().await;
    *lock(&fx.checker.decide_error) =
        Some(ordin_checker::CheckError::DatastoreFailed { retryable: true });
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
    let fx = Fx::new().await;
    *lock(&fx.inspector.workspaces_error) = Some(InspectError::StoreFailed { retryable: true });
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
    let fx = Fx::new().await;
    *lock(&fx.catalogue.find_error) = Some(("CAT_STORE_FAILED", true));
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
    let fx = Fx::new().await;
    *lock(&fx.catalogue.list_error) = Some(("CAT_STORE_FAILED", true));
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
}

#[tokio::test]
async fn the_record_rules_refuse_an_invalid_instance() {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts)[0].part.name = "x".repeat(121);
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "AGI_NAME_INVALID"
    );
    assert_eq!(fx.instance_count().await, 0);
    lock(&fx.catalogue.parts)[0].part.name = "Analyst".to_owned();
    lock(&fx.catalogue.parts)[0].part.version = String::new();
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "AGI_PACKAGE_VERSION_INVALID"
    );
    lock(&fx.catalogue.parts)[0].part.version = "1.2.0".to_owned();
    lock(&fx.catalogue.parts)[0].part.commit = "not a commit".to_owned();
    assert_eq!(
        fx.pc.add(fx.input()).await.unwrap_err().code,
        "AGI_COMMIT_INVALID"
    );
}

type Change = Box<dyn Fn(&mut InstanceRow)>;

fn valid_row() -> InstanceRow {
    InstanceRow {
        id: AgentInstanceId::new(),
        display_name: "Analyst".to_owned(),
        package_name: "analyst".to_owned(),
        package_version: "1.0.0".to_owned(),
        agent_repository_id: Some(ordin_types::AgentRepositoryId::new()),
        commit: Some(COMMIT_A.to_owned()),
        agent_folder_path: format!("{ROOT}data/agents/x/folder"),
        lock_file_path: "/data/agents/x/folder/ordin.lock".to_owned(),
        audit_result: Some(ordin_types::domain::agents::AuditResult::Passed),
        authority: ordin_types::domain::agents::Authority::DocumentOnly,
        parsed_declaration_ref: String::new(),
        is_trial_of_own_draft: false,
        status: AgentInstanceStatus::Ready,
        updated_between_runs_at: None,
        installed_at: Timestamp::now(),
        agent_identifier: AGENT_IDENTIFIER.to_owned(),
        agent_draft_id: None,
        stopped_reason: None,
        stopped_by: None,
        may_do: None,
        plan_hash: None,
        pending_choices: "[]".to_owned(),
        harness: ordin_types::domain::HarnessKind::ClaudeCode,
        workspace_languages: Vec::new(),
        interface_locale: "en-GB".to_owned(),
    }
}

#[test]
fn every_agi_rule_has_its_code() {
    use crate::catalogue::validate_instance;
    assert!(validate_instance(&valid_row()).is_ok());
    let cases: Vec<(&str, Change)> = vec![
        (
            "AGI_NAME_INVALID",
            Box::new(|r| r.display_name = String::new()),
        ),
        (
            "AGI_PACKAGE_NAME_INVALID",
            Box::new(|r| r.package_name = "Has Capitals".to_owned()),
        ),
        (
            "AGI_PACKAGE_NAME_INVALID",
            Box::new(|r| r.package_name = String::new()),
        ),
        (
            "AGI_PACKAGE_VERSION_INVALID",
            Box::new(|r| r.package_version = "v".repeat(65)),
        ),
        ("AGI_REPOSITORY_MISMATCH", Box::new(|r| r.commit = None)),
        (
            "AGI_COMMIT_INVALID",
            Box::new(|r| r.commit = Some("abc".to_owned())),
        ),
        (
            "AGI_FOLDER_INVALID",
            Box::new(|r| r.agent_folder_path = "relative/path".to_owned()),
        ),
        (
            "AGI_STATE_INCONSISTENT",
            Box::new(|r| {
                r.status = AgentInstanceStatus::Refused;
                r.audit_result = None;
            }),
        ),
        (
            "AGI_STATE_INCONSISTENT",
            Box::new(|r| r.stopped_reason = Some("why".to_owned())),
        ),
    ];
    for (code, change) in cases {
        let mut row = valid_row();
        change(&mut row);
        assert_eq!(validate_instance(&row).unwrap_err().code, code);
    }
    let mut refused = valid_row();
    refused.status = AgentInstanceStatus::Refused;
    refused.stopped_reason = Some("why".to_owned());
    refused.stopped_by = Some(StoppedBy::Trust);
    assert!(validate_instance(&refused).is_ok());
}

#[tokio::test]
async fn a_trial_replaces_the_first_trial_of_the_same_draft() {
    let fx = Fx::new().await;
    let input = fx.trial_input();
    let first = fx.pc.add_trial(input.clone()).await.expect("a trial");
    assert!(first.instance.is_trial_of_own_draft);
    assert_eq!(first.instance.agent_repository_id, None);
    assert_eq!(first.instance.commit, None);
    assert_eq!(first.workspaces, vec![fx.workspace]);
    {
        let requests = lock(&fx.resolver.requests);
        assert!(requests[0].allow_draft_root);
        assert!(requests[0].source.is_some());
    }
    assert!(lock(&fx.checker.checks)[0].source.is_some());
    let second = fx.pc.add_trial(input).await.expect("a second trial");
    assert_ne!(first.instance.id, second.instance.id);
    assert_eq!(
        fx.pc
            .get(first.instance.id)
            .await
            .expect("kept")
            .instance
            .status,
        AgentInstanceStatus::Removed
    );
    assert_eq!(fx.pc.list(None).await.expect("a list").len(), 1);
}

#[tokio::test]
async fn trial_failures_map_to_the_error_table() {
    let fx = Fx::new().await;
    *lock(&fx.catalogue.draft_error) = Some(("CAT_REPOSITORY_UNKNOWN", false));
    assert_eq!(
        fx.pc.add_trial(fx.trial_input()).await.unwrap_err().code,
        "PKG_LIBRARY_UNAVAILABLE"
    );
    *lock(&fx.catalogue.draft_error) = None;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::LoopMissing);
    assert_eq!(
        fx.pc.add_trial(fx.trial_input()).await.unwrap_err().code,
        "PKG_AGENT_NOT_OFFERED"
    );
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::SourceUnavailable { retryable: true });
    assert_eq!(
        fx.pc.add_trial(fx.trial_input()).await.unwrap_err().code,
        "PKG_CHECK_UNAVAILABLE"
    );
    *lock(&fx.resolver.resolve_error) = None;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    assert_eq!(
        fx.pc.add_trial(fx.trial_input()).await.unwrap_err().code,
        "PKG_AGENT_NOT_OFFERED"
    );
    *lock(&fx.checker.verdict) = InstallVerdict::Clean;
    *lock(&fx.checker.check_error) = Some(ordin_checker::CheckError::SourceNotBound);
    assert_eq!(
        fx.pc.add_trial(fx.trial_input()).await.unwrap_err().code,
        "PKG_CHECK_UNAVAILABLE"
    );
    *lock(&fx.checker.check_error) = None;
    *lock(&fx.catalogue.scope_error) = Some(("CAT_STORE_FAILED", true));
    assert_eq!(
        fx.pc.add_trial(fx.trial_input()).await.unwrap_err().code,
        "PKG_LIBRARY_UNAVAILABLE"
    );
}

#[tokio::test]
async fn a_trial_session_starts_with_the_bundled_scope() {
    let fx = Fx::new().await;
    let trial = fx.pc.add_trial(fx.trial_input()).await.expect("a trial");
    let ready = fx.pc.ensure_ready(trial.instance.id).await.expect("ready");
    assert_eq!(ready.instance.status, AgentInstanceStatus::Ready);
}

#[tokio::test]
async fn ensure_ready_repairs_and_journals() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.installers.repair) = Ok(true);
    fx.pc.ensure_ready(agent.instance.id).await.expect("ready");
    assert_eq!(fx.journal_kinds("agent_repaired").await, 1);
    *lock(&fx.installers.repair) = Ok(false);
    fx.pc.ensure_ready(agent.instance.id).await.expect("ready");
    assert_eq!(
        fx.journal_kinds("agent_repaired").await,
        1,
        "a folder that matched is not journalled"
    );
}

#[tokio::test]
async fn verify_agent_folders_reports_what_cannot_be_rebuilt() {
    let fx = Fx::new().await;
    fx.add().await;
    *lock(&fx.installers.verify) = Err(InstallError::NotInstalled);
    let report = fx.pc.verify_agent_folders(false).await.expect("a report");
    assert_eq!(report.failed[0].code, "PKG_FOLDER_UNUSABLE");
    *lock(&fx.installers.repair) = Err(InstallError::RepairFailed {
        code: "X".to_owned(),
    });
    let report = fx.pc.verify_agent_folders(true).await.expect("a report");
    assert_eq!((report.checked, report.repaired), (1, 0));
    assert_eq!(report.failed[0].code, "PKG_FOLDER_UNUSABLE");
    *lock(&fx.catalogue.scope_error) = Some(("CAT_STORE_FAILED", true));
    let report = fx.pc.verify_agent_folders(true).await.expect("a report");
    assert_eq!(report.failed[0].code, "PKG_FOLDER_UNUSABLE");
}

#[tokio::test]
async fn remove_from_one_workspace_keeps_the_instance_for_the_others() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let other = WorkspaceId::new();
    {
        let mut conn = fx.pool.acquire().await.expect("a connection");
        crate::store::add_membership(&mut conn, id, other)
            .await
            .expect("a membership");
    }
    fx.pc
        .remove(RemoveAgentInput {
            agent_instance_id: id,
            workspace_id: Some(fx.workspace),
        })
        .await
        .expect("dropped from one workspace");
    let kept = fx.pc.get(id).await.expect("still there");
    assert_eq!(kept.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(kept.workspaces, vec![other]);
    assert_eq!(
        lock(&fx.inspector.workspaces).last().cloned(),
        Some(vec![other])
    );
    // A workspace it does not serve.
    let error = fx
        .pc
        .remove(RemoveAgentInput {
            agent_instance_id: id,
            workspace_id: Some(fx.workspace),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, "PKG_INSTANCE_UNKNOWN");
    // The last workspace removes the instance.
    fx.pc
        .remove(RemoveAgentInput {
            agent_instance_id: id,
            workspace_id: Some(other),
        })
        .await
        .expect("removed");
    assert_eq!(
        fx.pc.get(id).await.expect("kept").instance.status,
        AgentInstanceStatus::Removed
    );
}

#[tokio::test]
async fn removal_tolerates_forget_failures_and_a_missing_install_and_folder() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.inspector.forget_error) = Some(InspectError::StoreFailed { retryable: true });
    *lock(&fx.checker.forget_error) =
        Some(ordin_checker::CheckError::DatastoreFailed { retryable: true });
    *lock(&fx.installers.uninstall_error) = Some(InstallError::NotInstalled);
    *lock(&fx.folders.remove_error) = Some(ordin_agent_folders::AfmError::FolderNotFound {
        id: agent.instance.id,
    });
    fx.pc
        .remove(RemoveAgentInput {
            agent_instance_id: agent.instance.id,
            workspace_id: None,
        })
        .await
        .expect("removed");
    assert_eq!(
        fx.pc
            .get(agent.instance.id)
            .await
            .expect("kept")
            .instance
            .status,
        AgentInstanceStatus::Removed
    );
}

#[tokio::test]
async fn removal_stops_when_the_installer_or_the_folder_cannot_be_undone() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let remove = RemoveAgentInput {
        agent_instance_id: agent.instance.id,
        workspace_id: None,
    };
    *lock(&fx.installers.uninstall_error) = Some(InstallError::WriteFailed { retryable: true });
    assert_eq!(
        fx.pc.remove(remove.clone()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
    *lock(&fx.installers.uninstall_error) = Some(InstallError::RecordInvalid);
    assert_eq!(
        fx.pc.remove(remove.clone()).await.unwrap_err().code,
        "PKG_FOLDER_UNUSABLE"
    );
    *lock(&fx.installers.uninstall_error) = None;
    *lock(&fx.folders.remove_error) =
        Some(ordin_agent_folders::AfmError::IoFailed { retryable: false });
    assert_eq!(
        fx.pc.remove(remove).await.unwrap_err().code,
        "PKG_FOLDER_UNUSABLE"
    );
    assert_eq!(
        fx.pc
            .get(agent.instance.id)
            .await
            .expect("kept")
            .instance
            .status,
        AgentInstanceStatus::Ready,
        "an instance that could not be undone is not marked removed"
    );
}

#[tokio::test]
async fn the_person_declines_an_update() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let declined = fx.pc.decline_update(id).await.expect("declined");
    assert_eq!(declined.instance.status, AgentInstanceStatus::Ready);
    assert!(declined.update.is_none());
    assert_eq!(declined.instance.package_version, "1.2.0");
    let kept = lock(&fx.resolver.pruned).last().cloned().expect("a prune");
    assert!(
        !kept.contains(&plan("b1", COMMIT_B, "1.3.0").plan_hash),
        "the candidate bundle is pruned"
    );
}

#[tokio::test]
async fn the_person_accepts_an_update_that_changes_what_the_agent_may_do() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    *lock(&fx.inspector.may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let accepted = fx.pc.accept_update(id).await.expect("accepted");
    assert_eq!(accepted.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(accepted.instance.package_version, "1.3.0");
    assert_eq!(
        accepted.instance.updated_between_runs_at, None,
        "the person was asked"
    );
    assert!(accepted.update.is_none());
    assert!(
        fx.pc.can_send_email(id).await,
        "the stored summary is the new one"
    );
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
}

#[tokio::test]
async fn accepting_waits_for_the_run_to_end() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    fx.activity.start(id);
    let pc = fx.pc.clone();
    let waiting = tokio::spawn(async move { pc.accept_update(id).await });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(count(&fx.log_lines(), "installers.update"), 0);
    fx.activity.finish(id);
    let done = waiting.await.expect("joined").expect("accepted");
    assert_eq!(done.instance.package_version, "1.3.0");
}

#[tokio::test]
async fn an_update_that_fails_its_checks_keeps_the_installed_version() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let kept = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(kept.instance.package_version, "1.2.0");
    assert_eq!(kept.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(fx.journal_kinds("agent_update_refused").await, 1);
}

#[tokio::test]
async fn an_update_that_needs_it_approval_asks_the_person() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("files")],
    };
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let waiting = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(
        waiting.instance.status,
        AgentInstanceStatus::UpdateAwaitingChoice
    );
}

#[tokio::test]
async fn an_unchanged_plan_and_unavailable_providers_change_nothing() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    // The head plan is the installed one.
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    assert_eq!(
        count(&fx.log_lines(), "resolver.build"),
        1,
        "only the install built a bundle"
    );
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::SourceUnavailable { retryable: true });
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.resolver.resolve_error) = None;
    *lock(&fx.resolver.build_error) = Some(ResolveError::StoreWriteFailed { retryable: true });
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.resolver.build_error) = None;
    *lock(&fx.checker.check_error) = Some(ordin_checker::CheckError::SourceNotBound);
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.checker.check_error) = None;
    *lock(&fx.catalogue.list_error) = Some(("CAT_STORE_FAILED", true));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.catalogue.list_error) = None;
    *lock(&fx.inspector.candidate_error) = Some(InspectError::NotInspected);
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.inspector.candidate_error) = None;
    // An install that fails puts the instance back as it was.
    *lock(&fx.installers.update_error) = Some(InstallError::WriteFailed { retryable: true });
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let same = fx.pc.get(id).await.expect("the instance");
    assert_eq!(same.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(same.instance.package_version, "1.2.0");
    assert_eq!(fx.journal_kinds("agent_update_refused").await, 1);
}

#[tokio::test]
async fn a_summary_that_cannot_be_read_asks_the_person() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    sqlx::query("UPDATE agent_instances SET may_do = 'not json'")
        .execute(&fx.pool)
        .await
        .expect("corrupt the column");
    assert!(
        !fx.pc.can_send_email(agent.instance.id).await,
        "an unreadable value is false"
    );
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let waiting = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(
        waiting.instance.status,
        AgentInstanceStatus::UpdateAwaitingChoice
    );
}

#[tokio::test]
async fn a_withdrawal_with_a_replacement_updates_the_agent() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let withdrawn = entry(&fx, Some("urn:ordin:acme:packs:go2"));
    lock(&fx.catalogue.withdrawn).push(withdrawn.clone());
    fx.set_plan(plan("r1", COMMIT_B, "1.3.0"));
    fx.pc
        .on_repository_updated(fx.repo, &[withdrawn.id], &shutdown())
        .await;
    {
        let requests = lock(&fx.resolver.requests);
        let replaced = requests
            .iter()
            .find(|r| !r.replacements.is_empty())
            .expect("a re-plan");
        assert_eq!(replaced.replacements[0].withdrawn_id, GO_PACK);
        assert_eq!(
            replaced.replacements[0].replacement_id,
            "urn:ordin:acme:packs:go2"
        );
    }
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(
        updated.instance.status,
        AgentInstanceStatus::Ready,
        "it is an update, not a stop"
    );
    assert_eq!(updated.instance.package_version, "1.3.0");
}

#[tokio::test]
async fn a_failing_replacement_stops_the_agent() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let withdrawn = entry(&fx, Some("urn:ordin:acme:packs:go2"));
    lock(&fx.catalogue.withdrawn).push(withdrawn.clone());
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    fx.set_plan(plan("r1", COMMIT_B, "1.3.0"));
    *lock(&fx.resolver.build_error) = Some(ResolveError::PartWithdrawn {
        part: GO_PACK.to_owned(),
        replacement: None,
    });
    fx.pc
        .on_repository_updated(fx.repo, &[withdrawn.id], &shutdown())
        .await;
    let stopped = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(stopped.instance.status, AgentInstanceStatus::Refused);
}

#[tokio::test]
async fn a_withdrawal_that_names_no_part_of_the_agent_changes_nothing() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let mut other = entry(&fx, None);
    other.part_identifier = "urn:ordin:acme:packs:unrelated".to_owned();
    let mut other_version = entry(&fx, None);
    other_version.part_version = Some("9.9.9".to_owned());
    lock(&fx.catalogue.withdrawn).extend([other.clone(), other_version.clone()]);
    fx.pc
        .on_repository_updated(
            fx.repo,
            &[other.id, other_version.id, WithdrawnListEntryId::new()],
            &shutdown(),
        )
        .await;
    assert_eq!(
        fx.pc
            .get(agent.instance.id)
            .await
            .expect("the instance")
            .instance
            .status,
        AgentInstanceStatus::Ready
    );
    // An unreadable withdrawn list is logged and changes nothing.
    *lock(&fx.catalogue.withdrawn_error) = true;
    fx.pc
        .on_repository_updated(fx.repo, &[other.id], &shutdown())
        .await;
    *lock(&fx.catalogue.withdrawn_error) = false;
    *lock(&fx.catalogue.scope_error) = Some(("CAT_STORE_FAILED", true));
    fx.pc
        .on_repository_updated(fx.repo, &[other.id], &shutdown())
        .await;
    *lock(&fx.catalogue.scope_error) = None;
    *lock(&fx.resolver.open_error) = true;
    fx.pc
        .on_repository_updated(fx.repo, &[other.id], &shutdown())
        .await;
    assert_eq!(
        fx.pc
            .get(agent.instance.id)
            .await
            .expect("the instance")
            .instance
            .status,
        AgentInstanceStatus::Ready
    );
}

#[tokio::test]
async fn a_withdrawal_stops_the_agent_with_the_repository_it_came_from() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let withdrawn = entry(&fx, None);
    lock(&fx.catalogue.withdrawn).push(withdrawn.clone());
    *lock(&fx.catalogue.list_error) = Some(("CAT_STORE_FAILED", true));
    fx.pc
        .on_repository_updated(fx.repo, &[withdrawn.id], &shutdown())
        .await;
    let stopped = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(
        stopped.instance.stopped_reason.as_deref(),
        Some("Analyst stopped: go was withdrawn by its repository.")
    );
}

#[tokio::test]
async fn a_fixed_version_that_cannot_be_used_changes_nothing() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let withdrawn = entry(&fx, None);
    lock(&fx.catalogue.withdrawn).push(withdrawn.clone());
    *lock(&fx.resolver.build_error) = Some(ResolveError::PartWithdrawn {
        part: GO_PACK.to_owned(),
        replacement: None,
    });
    fx.pc
        .on_repository_updated(fx.repo, &[withdrawn.id], &shutdown())
        .await;
    *lock(&fx.resolver.build_error) = None;
    fx.set_plan(plan("c1", COMMIT_B, "1.3.0"));
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.checker.verdict) = InstallVerdict::Clean;
    *lock(&fx.checker.check_error) = Some(ordin_checker::CheckError::SourceNotBound);
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.checker.check_error) = None;
    *lock(&fx.inspector.candidate_error) = Some(InspectError::NotInspected);
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    assert!(fx.pc.get(id).await.expect("the instance").update.is_none());
    // Declining leaves the stopped instance stopped.
    *lock(&fx.inspector.candidate_error) = None;
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let declined = fx.pc.decline_update(id).await.expect("declined");
    assert_eq!(declined.instance.status, AgentInstanceStatus::Refused);
    assert!(declined.update.is_none());
}

#[tokio::test]
async fn an_accepted_update_that_cannot_be_installed_is_refused_and_journalled() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    *lock(&fx.installers.update_error) = Some(InstallError::WriteFailed { retryable: true });
    let error = fx.pc.accept_update(agent.instance.id).await.unwrap_err();
    assert_eq!(error.code, "PKG_INSTALL_FAILED");
    let waiting = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(
        waiting.instance.status,
        AgentInstanceStatus::UpdateAwaitingChoice
    );
    *lock(&fx.resolver.open_error) = true;
    assert_eq!(
        fx.pc
            .accept_update(agent.instance.id)
            .await
            .unwrap_err()
            .code,
        "PKG_INSTALL_FAILED"
    );
}

#[tokio::test]
async fn the_loop_applies_a_repository_update_and_stops_on_shutdown() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let token = shutdown();
    let pc = fx.pc.clone();
    let stop = token.clone();
    let running = tokio::spawn(async move { pc.run(stop).await });
    // The loop subscribes before it reads; wait until it has.
    fx.wait_until(|| async { fx.catalogue.tx.receiver_count() > 0 })
        .await;
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.catalogue
        .tx
        .send(CatalogueNotice::RepositoryUpdated {
            agent_repository_id: fx.repo,
            commit: COMMIT_B.to_owned(),
            newly_withdrawn: Vec::new(),
        })
        .expect("a subscriber");
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.status == AgentInstanceStatus::UpdateAwaitingChoice)
    })
    .await;
    // A dropped repository is not offered again.
    fx.catalogue
        .tx
        .send(CatalogueNotice::RepositoryDropped {
            agent_repository_id: fx.repo,
        })
        .expect("a subscriber");
    fx.wait_until(|| async {
        fx.pc
            .inner
            .dropped
            .lock()
            .is_ok_and(|d| d.contains(&fx.repo))
    })
    .await;
    token.cancel();
    running
        .await
        .expect("joined")
        .expect("the loop ends cleanly");
}

#[tokio::test]
async fn a_restart_finds_again_an_update_that_was_waiting_for_a_run_to_end() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    // The update changes nothing the agent may do, but a run is going, so it waits in memory.
    fx.activity.start(id);
    let first_start = shutdown();
    fx.pc
        .on_repository_updated(fx.repo, &[], &first_start)
        .await;
    // The program stops before the run ends: the waiting update is gone with it.
    first_start.cancel();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    fx.activity.finish(id);
    assert_eq!(
        fx.pc.get(id).await.unwrap().instance.package_version,
        "1.2.0"
    );
    // The next start sweeps the repositories again, with no notice.
    let token = shutdown();
    let pc = fx.pc.clone();
    let stop = token.clone();
    let running = tokio::spawn(async move { pc.run(stop).await });
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.package_version == "1.3.0")
    })
    .await;
    token.cancel();
    running
        .await
        .expect("joined")
        .expect("the loop ends cleanly");
}

#[tokio::test]
async fn a_dropped_repository_is_skipped_by_updates() {
    let fx = Fx::new().await;
    fx.add().await;
    fx.pc.inner.dropped.lock().expect("lock").insert(fx.repo);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    assert_eq!(count(&fx.log_lines(), "resolver.build"), 1);
}

#[tokio::test]
async fn a_failed_choice_restores_the_earlier_choice() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let by_change = {
        let mut p = plan("a2", COMMIT_A, "1.2.0");
        p.selections
            .practice_packs
            .push(ordin_resolver::model::SelectedPart {
                id: PYTEST_PACK.to_owned(),
                chosen_by: ordin_resolver::model::ChosenBy::PersonChange,
            });
        p
    };
    *lock(&fx.resolver.plan_fn) = Some(Arc::new(move |_| Ok(by_change.clone())));
    let change = ChangeChoiceInput {
        agent_instance_id: id,
        choice: PackChoice {
            rule_index: 0,
            packs: vec![PYTEST_PACK.to_owned()],
        },
    };
    *lock(&fx.installers.update_error) = Some(InstallError::WriteFailed { retryable: true });
    assert_eq!(
        fx.pc.change_choice(change.clone()).await.unwrap_err().code,
        "PKG_INSTALL_FAILED"
    );
    let left = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_instance_choices")
        .fetch_one(&fx.pool)
        .await
        .expect("count");
    assert_eq!(left, 0, "a choice that could not be installed is not kept");
    // A stored earlier choice comes back.
    *lock(&fx.installers.update_error) = None;
    fx.pc.change_choice(change.clone()).await.expect("changed");
    *lock(&fx.installers.update_error) = Some(InstallError::WriteFailed { retryable: true });
    let again = ChangeChoiceInput {
        agent_instance_id: id,
        choice: PackChoice {
            rule_index: 0,
            packs: vec![PYTEST_PACK.to_owned(), GO_PACK.to_owned()],
        },
    };
    assert!(fx.pc.change_choice(again).await.is_err());
    let packs = sqlx::query_scalar::<_, String>(
        "SELECT packs FROM agent_instance_choices WHERE rule_index = 0",
    )
    .fetch_one(&fx.pool)
    .await
    .expect("the stored choice");
    assert_eq!(packs, format!("[\"{PYTEST_PACK}\"]"));
}

#[tokio::test]
async fn a_choice_that_fails_checks_or_cannot_be_planned_is_not_allowed() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let change = ChangeChoiceInput {
        agent_instance_id: agent.instance.id,
        choice: PackChoice {
            rule_index: 0,
            packs: vec![PYTEST_PACK.to_owned()],
        },
    };
    let by_change = {
        let mut p = plan("a2", COMMIT_A, "1.2.0");
        p.selections
            .practice_packs
            .push(ordin_resolver::model::SelectedPart {
                id: PYTEST_PACK.to_owned(),
                chosen_by: ordin_resolver::model::ChosenBy::PersonChange,
            });
        p
    };
    *lock(&fx.resolver.plan_fn) = Some(Arc::new(move |_| Ok(by_change.clone())));
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    assert_eq!(
        fx.pc.change_choice(change.clone()).await.unwrap_err().code,
        "PKG_CHOICE_NOT_ALLOWED"
    );
    *lock(&fx.checker.verdict) = InstallVerdict::Clean;
    *lock(&fx.checker.check_error) = Some(ordin_checker::CheckError::SourceNotBound);
    assert_eq!(
        fx.pc.change_choice(change.clone()).await.unwrap_err().code,
        "PKG_CHECK_UNAVAILABLE"
    );
    *lock(&fx.checker.check_error) = None;
    *lock(&fx.resolver.plan_fn) = None;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::SourceUnavailable { retryable: true });
    assert_eq!(
        fx.pc.change_choice(change.clone()).await.unwrap_err().code,
        "PKG_CHECK_UNAVAILABLE"
    );
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::LoopMissing);
    assert_eq!(
        fx.pc.change_choice(change.clone()).await.unwrap_err().code,
        "PKG_CHOICE_NOT_ALLOWED"
    );
    *lock(&fx.resolver.resolve_error) = None;
    let empty = ChangeChoiceInput {
        agent_instance_id: agent.instance.id,
        choice: PackChoice {
            rule_index: 0,
            packs: Vec::new(),
        },
    };
    assert_eq!(
        fx.pc.change_choice(empty).await.unwrap_err().code,
        "PKG_CHOICE_NOT_ALLOWED"
    );
    // A stopped instance cannot change its packs; a trial has no scope to re-plan in.
    let trial = fx.pc.add_trial(fx.trial_input()).await.expect("a trial");
    let on_trial = ChangeChoiceInput {
        agent_instance_id: trial.instance.id,
        ..change
    };
    assert_eq!(
        fx.pc.change_choice(on_trial).await.unwrap_err().code,
        "PKG_TRIAL_NO_SCOPE"
    );
}

#[tokio::test]
async fn the_stored_choices_reach_the_next_plan() {
    let fx = Fx::new().await;
    let mut input = fx.input();
    input.choices = vec![PackChoice {
        rule_index: 0,
        packs: vec![PYTEST_PACK.to_owned()],
    }];
    fx.pc.preview_add(input.clone()).await.expect("a preview");
    assert_eq!(
        lock(&fx.resolver.requests)[0].pack_changes,
        vec![PYTEST_PACK.to_owned()]
    );
    let agent = match fx.pc.add(input).await.expect("added") {
        AddOutcome::Ready { agent } => agent,
        other => panic!("{other:?}"),
    };
    let mut conn = fx.pool.acquire().await.expect("a connection");
    let stored = crate::store::choices_of(&mut conn, agent.instance.id)
        .await
        .expect("choices");
    assert_eq!(
        stored,
        vec![StoredChoice {
            rule_index: 0,
            packs: vec![PYTEST_PACK.to_owned()],
            source: ChoiceSource::PersonChange
        }]
    );
    // Languages and locale are kept with the instance for every later re-plan.
    drop(conn);
    assert_eq!(
        fx.column(agent.instance.id, "workspace_languages")
            .await
            .as_deref(),
        Some("[\"go\"]")
    );
    assert_eq!(
        fx.column(agent.instance.id, "interface_locale")
            .await
            .as_deref(),
        Some("en-GB")
    );
}

#[tokio::test]
async fn readers_cope_with_unreadable_providers() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.inspector.model_error) = Some(InspectError::NotInspected);
    *lock(&fx.resolver.open_error) = true;
    let read = fx
        .pc
        .get(agent.instance.id)
        .await
        .expect("an instance without tasks and uses");
    assert!(read.tasks.is_empty());
    assert!(read.uses.is_empty());
    let listed = fx.pc.list(Some(fx.workspace)).await.expect("a list");
    assert_eq!(listed.len(), 1);
    *lock(&fx.catalogue.repo_ref_error) = Some(("CAT_STORE_FAILED", true));
    assert_eq!(
        fx.pc
            .agent_identity(agent.instance.id)
            .await
            .unwrap_err()
            .code,
        "PKG_LIBRARY_UNAVAILABLE"
    );
    *lock(&fx.catalogue.repo_ref_error) = None;
    let (reference, identifier, version, name) = fx
        .pc
        .agent_identity(agent.instance.id)
        .await
        .expect("an identity");
    assert_eq!(
        reference,
        crate::types::AgentRepositoryRef::ProfileKey("acme-agents".to_owned())
    );
    assert_eq!(
        (identifier.as_str(), version.as_str(), name.as_str()),
        (AGENT_IDENTIFIER, "1.2.0", "Analyst")
    );
}

#[tokio::test]
async fn lookups_map_library_failures() {
    let fx = Fx::new().await;
    *lock(&fx.catalogue.by_ref) = Err(("CAT_STORE_FAILED", true));
    let key = crate::types::AgentRepositoryRef::ProfileKey("acme-agents".to_owned());
    assert_eq!(
        fx.pc
            .match_workflow_agent(fx.workspace, &key, AGENT_IDENTIFIER, "1")
            .await
            .unwrap_err()
            .code,
        "PKG_LIBRARY_UNAVAILABLE"
    );
    *lock(&fx.catalogue.find_error) = Some(("CAT_REPOSITORY_UNKNOWN", false));
    assert_eq!(
        fx.pc
            .check_proposal(fx.repo, AGENT_IDENTIFIER)
            .await
            .unwrap_err()
            .code,
        "PKG_AGENT_UNKNOWN"
    );
    *lock(&fx.catalogue.find_error) = Some(("CAT_STORE_FAILED", true));
    assert_eq!(
        fx.pc
            .check_proposal(fx.repo, AGENT_IDENTIFIER)
            .await
            .unwrap_err()
            .code,
        "PKG_LIBRARY_UNAVAILABLE"
    );
    *lock(&fx.catalogue.find_error) = None;
    assert!(fx
        .pc
        .check_proposal(fx.repo, AGENT_IDENTIFIER)
        .await
        .is_ok());
    lock(&fx.catalogue.parts)[0].state_label = ordin_catalogue::PartStateLabel::NotForNewAgents;
    assert_eq!(
        fx.pc
            .check_proposal(fx.repo, AGENT_IDENTIFIER)
            .await
            .unwrap_err()
            .code,
        "PKG_DEPRECATED_AS_AGENT"
    );
    lock(&fx.catalogue.parts)[0].offered = false;
    lock(&fx.catalogue.parts)[0].state_label = ordin_catalogue::PartStateLabel::Current;
    assert_eq!(
        fx.pc
            .check_proposal(fx.repo, AGENT_IDENTIFIER)
            .await
            .unwrap_err()
            .code,
        "PKG_AGENT_NOT_OFFERED"
    );
}

#[tokio::test]
async fn pending_choice_labels_fall_back_to_the_identifier() {
    let fx = Fx::new().await;
    let mut asking = plan("a1", COMMIT_A, "1.2.0");
    asking.selections.pending_choices =
        vec![pending(1, "style", &["urn:ordin:acme:packs:unknown"]), {
            let mut no_class = pending(0, "x", &[PYTEST_PACK]);
            no_class.class = None;
            no_class
        }];
    fx.set_plan(asking);
    let agent = fx.add().await;
    let open = fx
        .pc
        .pending_choices(agent.instance.id)
        .await
        .expect("questions");
    assert_eq!(
        open.iter().map(|o| o.rule_index).collect::<Vec<_>>(),
        vec![0, 1],
        "lowest rule first"
    );
    assert_eq!(open[0].class_label, "pack");
    assert_eq!(open[1].choices[0].label, "urn:ordin:acme:packs:unknown");
    // A trial has no repository to look the names up in.
    let trial = fx.pc.add_trial(fx.trial_input()).await.expect("a trial");
    let open = fx
        .pc
        .pending_choices(trial.instance.id)
        .await
        .expect("questions");
    assert_eq!(open[0].choices[0].label, open[0].choices[0].pack);
}

#[tokio::test]
async fn a_pending_update_is_decoded_into_the_inspectors_words() {
    let changes = vec![
        MayDoChange {
            kind: MayDoChangeKind::PolicyChanged,
            name: String::new(),
            message_id: "may-do-policy-changed",
        },
        MayDoChange {
            kind: MayDoChangeKind::AuthorityChanged,
            name: "system_changing".to_owned(),
            message_id: "may-do-authority-changed",
        },
    ];
    let text = serde_json::to_string(&changes).expect("json");
    assert_eq!(
        crate::views::decode_changes(&text).expect("decoded"),
        changes
    );
    for kind in [
        MayDoChangeKind::ToolAdded,
        MayDoChangeKind::ToolRemoved,
        MayDoChangeKind::ReadAdded,
        MayDoChangeKind::ReadRemoved,
        MayDoChangeKind::ConnectionAdded,
        MayDoChangeKind::ConnectionRemoved,
        MayDoChangeKind::DeclarationChanged,
    ] {
        let one = serde_json::to_string(&vec![MayDoChange {
            kind,
            name: "x".to_owned(),
            message_id: "ignored",
        }])
        .expect("json");
        let decoded = crate::views::decode_changes(&one).expect("decoded");
        assert!(decoded[0].message_id.starts_with("may-do-"));
    }
    assert_eq!(
        crate::views::decode_changes("nope").unwrap_err().code,
        "PKG_STORE_FAILED"
    );
}

#[tokio::test]
async fn the_inputs_refuse_unknown_fields() {
    let fx = Fx::new().await;
    let mut value = serde_json::to_value(fx.input()).expect("json");
    assert!(serde_json::from_value::<crate::types::AddAgentInput>(value.clone()).is_ok());
    value["surprise"] = serde_json::json!(1);
    assert!(serde_json::from_value::<crate::types::AddAgentInput>(value).is_err());
    assert!(serde_json::from_str::<RemoveAgentInput>(&format!(
        "{{\"agent_instance_id\":\"{}\",\"extra\":1}}",
        AgentInstanceId::new()
    ))
    .is_err());
    let outcome = serde_json::to_value(crate::types::AddOutcome::NeedsItApproval {
        parts: vec!["files".to_owned()],
    })
    .expect("json");
    assert_eq!(outcome["kind"], "needs_it_approval");
    assert_eq!(
        serde_json::to_value(Availability::PackMissing {
            pack_class_label: "testing".to_owned()
        })
        .expect("json")["kind"],
        "pack_missing"
    );
}

#[tokio::test]
async fn activity_and_harness_are_read_through_the_programs_traits() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    fx.activity.start(id);
    assert!(fx.pc.inner.activity.running(id));
    let mut idle = fx.pc.inner.activity.idle();
    fx.activity.finish(id);
    assert_eq!(idle.recv().await.expect("idle"), id);
    assert_eq!(
        fx.pc.inner.harness.default_harness(),
        ordin_types::domain::HarnessKind::ClaudeCode
    );
    assert!(matches!(
        AgentNotice::Removed {
            agent_instance_id: id
        },
        AgentNotice::Removed { .. }
    ));
}
