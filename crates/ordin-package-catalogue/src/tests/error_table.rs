//! X1: one test per row of PackageCatalogue.md *Error Table*.

use ordin_catalogue::PartStateLabel;
use ordin_checker::{CheckError, InstallVerdict, TrustChanges};
use ordin_installers::{InstallError, VerifyOutcome};
use ordin_resolver::model::PartKind;
use ordin_resolver::ResolveError;
use ordin_types::{AgentInstanceId, RepairAction};

use super::support::*;
use crate::error::PackageError;
use crate::types::{AddOutcome, ChangeChoiceInput, PackChoice, RemoveAgentInput};

fn assert_error(error: &PackageError, code: &str, retryable: bool, action: Option<RepairAction>) {
    assert_eq!(error.code, code);
    assert_eq!(error.retryable, retryable, "{code} retryable");
    assert_eq!(error.action, action, "{code} action");
    assert_eq!(error.to_string(), code, "Display is the bare code");
}

/// Marks the one instance `refused` by a trust loss.
async fn refuse_by_trust(fx: &Fx, id: AgentInstanceId) {
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: vec![id],
            gained: Vec::new(),
        })
        .await
        .expect("trust changes apply");
}

#[tokio::test]
async fn agent_part_unknown() {
    let fx = Fx::new().await;
    *lock(&fx.catalogue.part_error) = Some(("CAT_PART_UNKNOWN", false));
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_AGENT_UNKNOWN",
        false,
        None,
    );
    assert_error(
        &fx.pc.preview_add(fx.input()).await.unwrap_err(),
        "PKG_AGENT_UNKNOWN",
        false,
        None,
    );
}

#[tokio::test]
async fn part_is_not_an_agent() {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts)[0].part.kind = PartKind::Rule;
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_NOT_AN_AGENT",
        false,
        None,
    );
}

#[tokio::test]
async fn agent_not_offered_failed_checks_withdrawn() {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts)[0].offered = false;
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_AGENT_NOT_OFFERED",
        false,
        None,
    );
    {
        let mut parts = lock(&fx.catalogue.parts);
        parts[0].offered = true;
        parts[0].state_label = PartStateLabel::Withdrawn;
    }
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_AGENT_NOT_OFFERED",
        false,
        None,
    );
}

#[tokio::test]
async fn deprecated_part_added_as_an_agent() {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts)[0].state_label = PartStateLabel::NotForNewAgents;
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_DEPRECATED_AS_AGENT",
        false,
        None,
    );
}

#[tokio::test]
async fn already_added_to_this_workspace() {
    let fx = Fx::new().await;
    fx.add().await;
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_ALREADY_ADDED",
        false,
        None,
    );
}

#[tokio::test]
async fn checks_fail() {
    let fx = Fx::new().await;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    let outcome = fx
        .pc
        .add(fx.input())
        .await
        .expect("an outcome, not an error");
    assert!(matches!(outcome, AddOutcome::Unsafe), "{outcome:?}");
    assert_eq!(fx.instance_count().await, 0, "nothing is created");
    assert_eq!(count(&fx.log_lines(), "folders.create"), 0);
    assert_eq!(fx.journal_kinds("agent_add_refused").await, 1);
}

#[tokio::test]
async fn outside_the_trust_policy() {
    let fx = Fx::new().await;
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("files")],
    };
    let outcome = fx.pc.add(fx.input()).await.expect("an outcome");
    match outcome {
        AddOutcome::NeedsItApproval { parts } => assert_eq!(parts, vec!["files".to_owned()]),
        other => panic!("{other:?}"),
    }
    assert_eq!(fx.instance_count().await, 0);
    assert_eq!(count(&fx.log_lines(), "folders.create"), 0);
}

#[tokio::test]
async fn workspace_cannot_settle_a_required_pack() {
    let fx = Fx::new().await;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::RequiredPackUnsettled {
        rule_index: 0,
        class: Some("testing".to_owned()),
    });
    match fx.pc.add(fx.input()).await.expect("an outcome") {
        AddOutcome::PackMissing { pack_class_label } => assert_eq!(pack_class_label, "testing"),
        other => panic!("{other:?}"),
    }
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::RequiredPackUnsettled {
        rule_index: 0,
        class: None,
    });
    match fx.pc.add(fx.input()).await.expect("an outcome") {
        AddOutcome::PackMissing { pack_class_label } => assert_eq!(pack_class_label, "pack"),
        other => panic!("{other:?}"),
    }
    assert_eq!(fx.instance_count().await, 0);
}

#[tokio::test]
async fn checker_unavailable() {
    let fx = Fx::new().await;
    *lock(&fx.checker.check_error) = Some(CheckError::SourceUnavailable { retryable: true });
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_CHECK_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    // A retryable resolution error is the same condition (§A0.3).
    *lock(&fx.checker.check_error) = None;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::SourceUnavailable { retryable: true });
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_CHECK_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

#[tokio::test]
async fn build_install_or_inspection_failed_twice() {
    let fx = Fx::new().await;
    fx.resolver
        .build_failures
        .store(2, std::sync::atomic::Ordering::SeqCst);
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_INSTALL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    let id = fx.only_instance_id().await;
    assert_eq!(fx.column(id, "status").await.as_deref(), Some("refused"));
    assert_eq!(
        fx.column(id, "audit_result").await.as_deref(),
        Some("failed")
    );
    assert_eq!(
        count(&fx.log_lines(), "folders.remove"),
        2,
        "each attempt's folder is removed"
    );
    // Inspection failing twice is the same row; a declaration the inspector refuses counts too.
    let fx = Fx::new().await;
    *lock(&fx.inspector.inspect_error) =
        Some(ordin_package_inspector::InspectError::DeclarationRefused {
            code: "PKI_DECLARATION_REFUSED",
            retryable: false,
        });
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_INSTALL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    assert_eq!(
        count(&fx.log_lines(), "installers.uninstall"),
        2,
        "the installer was reached twice"
    );
}

#[tokio::test]
async fn choice_outside_what_the_agent_allows() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.resolver.resolve_error) = Some(ResolveError::PackNotAllowed {
        id: "urn:ordin:acme:packs:other".to_owned(),
    });
    let input = ChangeChoiceInput {
        agent_instance_id: agent.instance.id,
        choice: PackChoice {
            rule_index: 0,
            packs: vec!["urn:ordin:acme:packs:other".to_owned()],
        },
    };
    assert_error(
        &fx.pc.change_choice(input.clone()).await.unwrap_err(),
        "PKG_CHOICE_NOT_ALLOWED",
        false,
        None,
    );
    assert_error(
        &fx.pc.record_answer(input).await.unwrap_err(),
        "PKG_CHOICE_NOT_ALLOWED",
        false,
        None,
    );
    // A rule the agent does not have: the plan does not use the chosen pack.
    *lock(&fx.resolver.resolve_error) = None;
    let missing = ChangeChoiceInput {
        agent_instance_id: agent.instance.id,
        choice: PackChoice {
            rule_index: 9,
            packs: vec![PYTEST_PACK.to_owned()],
        },
    };
    assert_error(
        &fx.pc.change_choice(missing).await.unwrap_err(),
        "PKG_CHOICE_NOT_ALLOWED",
        false,
        None,
    );
    // The same rule chosen twice on an add.
    let fx = Fx::new().await;
    let mut input = fx.input();
    let choice = PackChoice {
        rule_index: 0,
        packs: vec![GO_PACK.to_owned()],
    };
    input.choices = vec![choice.clone(), choice];
    assert_error(
        &fx.pc.add(input).await.unwrap_err(),
        "PKG_CHOICE_NOT_ALLOWED",
        false,
        None,
    );
}

#[tokio::test]
async fn no_update_waiting() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    assert_error(
        &fx.pc.accept_update(agent.instance.id).await.unwrap_err(),
        "PKG_NO_UPDATE",
        false,
        None,
    );
    assert_error(
        &fx.pc.decline_update(agent.instance.id).await.unwrap_err(),
        "PKG_NO_UPDATE",
        false,
        None,
    );
}

#[tokio::test]
async fn instance_unknown_or_removed() {
    let fx = Fx::new().await;
    let unknown = AgentInstanceId::new();
    assert_error(
        &fx.pc.get(unknown).await.unwrap_err(),
        "PKG_INSTANCE_UNKNOWN",
        false,
        None,
    );
    assert_error(
        &fx.pc
            .remove(RemoveAgentInput {
                agent_instance_id: unknown,
                workspace_id: None,
            })
            .await
            .unwrap_err(),
        "PKG_INSTANCE_UNKNOWN",
        false,
        None,
    );
    let agent = fx.add().await;
    let remove = RemoveAgentInput {
        agent_instance_id: agent.instance.id,
        workspace_id: None,
    };
    fx.pc.remove(remove.clone()).await.expect("removed");
    assert_error(
        &fx.pc.remove(remove).await.unwrap_err(),
        "PKG_INSTANCE_UNKNOWN",
        false,
        None,
    );
    assert_error(
        &fx.pc.source_scope(agent.instance.id).await.unwrap_err(),
        "PKG_INSTANCE_UNKNOWN",
        false,
        None,
    );
    assert_error(
        &fx.pc.agent_identity(agent.instance.id).await.unwrap_err(),
        "PKG_INSTANCE_UNKNOWN",
        false,
        None,
    );
    assert_error(
        &fx.pc.ensure_ready(unknown).await.unwrap_err(),
        "PKG_INSTANCE_UNKNOWN",
        false,
        None,
    );
}

#[tokio::test]
async fn instance_not_ready_for_a_session_or_pending_choices_asked_for_an_instance_not_ready_updating(
) {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    refuse_by_trust(&fx, agent.instance.id).await;
    assert_error(
        &fx.pc.ensure_ready(agent.instance.id).await.unwrap_err(),
        "PKG_INSTANCE_NOT_READY",
        false,
        None,
    );
    assert_error(
        &fx.pc.pending_choices(agent.instance.id).await.unwrap_err(),
        "PKG_INSTANCE_NOT_READY",
        false,
        None,
    );
}

#[tokio::test]
async fn folder_cannot_be_verified_or_rebuilt() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    *lock(&fx.installers.repair) = Err(InstallError::RepairFailed {
        code: "RES_LOCK_PART_UNAVAILABLE".to_owned(),
    });
    assert_error(
        &fx.pc.ensure_ready(agent.instance.id).await.unwrap_err(),
        "PKG_FOLDER_UNUSABLE",
        false,
        None,
    );
    // A retryable installer error is a failed install the person can try again.
    *lock(&fx.installers.repair) = Err(InstallError::WriteFailed { retryable: true });
    assert_error(
        &fx.pc.ensure_ready(agent.instance.id).await.unwrap_err(),
        "PKG_INSTALL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
}

#[tokio::test]
async fn trial_needs_it_approval() {
    let fx = Fx::new().await;
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("files")],
    };
    assert_error(
        &fx.pc.add_trial(fx.trial_input()).await.unwrap_err(),
        "PKG_TRIAL_NEEDS_TRUST",
        false,
        None,
    );
    assert_eq!(fx.instance_count().await, 0);
}

#[tokio::test]
async fn folder_differs_from_its_lock_file_verify_agent_folders_without_repair() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    lock(&fx.installers.verify_for).insert(
        agent.instance.id,
        VerifyOutcome::Mismatch {
            changed: vec!["AGENTS.md".to_owned()],
            missing: Vec::new(),
        },
    );
    let report = fx.pc.verify_agent_folders(false).await.expect("a report");
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].code, "PKG_FOLDER_MISMATCH");
    assert_eq!(report.failed[0].agent_name, "Analyst");
}

#[tokio::test]
async fn two_instances_in_the_workspace_share_the_name_a_format_1_2_workflow_cites() {
    let fx = Fx::new().await;
    fx.add().await;
    let other = fx.second_agent("urn:ordin:acme:agents:analyst-two", "analyst ");
    fx.add_agent(other).await;
    assert_error(
        &fx.pc
            .match_by_name(fx.workspace, " ANALYST")
            .await
            .unwrap_err(),
        "PKG_AGENT_NAME_AMBIGUOUS",
        false,
        None,
    );
}

#[tokio::test]
async fn identity_asked_for_a_trial_of_an_own_draft() {
    let fx = Fx::new().await;
    let trial = fx.pc.add_trial(fx.trial_input()).await.expect("a trial");
    assert_error(
        &fx.pc.agent_identity(trial.instance.id).await.unwrap_err(),
        "PKG_TRIAL_NOT_NAMEABLE",
        false,
        None,
    );
}

#[tokio::test]
async fn source_scope_asked_for_a_trial_of_an_own_draft() {
    let fx = Fx::new().await;
    let trial = fx.pc.add_trial(fx.trial_input()).await.expect("a trial");
    assert_error(
        &fx.pc.source_scope(trial.instance.id).await.unwrap_err(),
        "PKG_TRIAL_NO_SCOPE",
        false,
        None,
    );
}

#[tokio::test]
async fn trust_regained_but_the_folder_no_longer_matches_its_lock_file() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    refuse_by_trust(&fx, agent.instance.id).await;
    *lock(&fx.installers.verify) = Ok(VerifyOutcome::Mismatch {
        changed: vec!["AGENTS.md".to_owned()],
        missing: Vec::new(),
    });
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: Vec::new(),
            gained: vec![agent.instance.id],
        })
        .await
        .expect("no error is raised");
    let after = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(
        after.instance.status,
        ordin_types::domain::agents::AgentInstanceStatus::Refused
    );
    // An installer error is the same: the instance stays refused.
    *lock(&fx.installers.verify) = Err(InstallError::NotInstalled);
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: Vec::new(),
            gained: vec![agent.instance.id],
        })
        .await
        .expect("no error is raised");
    let still = fx.pc.get(agent.instance.id).await.expect("the instance");
    assert_eq!(
        still.instance.status,
        ordin_types::domain::agents::AgentInstanceStatus::Refused
    );
    assert!(still.instance.stopped_reason.is_some());
}

#[tokio::test]
async fn find_agents_need_empty_or_over_500_characters() {
    let fx = Fx::new().await;
    for need in ["", "   ", &"x".repeat(501)] {
        assert_error(
            &fx.pc.find_agents(fx.workspace, need).await.unwrap_err(),
            "PKG_NEED_INVALID",
            false,
            None,
        );
    }
    assert!(fx
        .pc
        .find_agents(fx.workspace, &"x".repeat(500))
        .await
        .is_ok());
}

#[tokio::test]
async fn catalogue_service_search_failed() {
    let fx = Fx::new().await;
    *lock(&fx.catalogue.search_error) = Some(("CAT_STORE_FAILED", true));
    let error = fx
        .pc
        .find_agents(fx.workspace, "compare")
        .await
        .unwrap_err();
    assert_eq!(error.code, "PKG_LIBRARY_UNAVAILABLE");
    assert!(error.retryable);
}

#[tokio::test]
async fn apply_trust_changes_names_an_instance_that_is_not_running_or_unknown() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let mut notices = fx.pc.subscribe();
    refuse_by_trust(&fx, agent.instance.id).await;
    let first = notices.try_recv().expect("the first loss is announced");
    assert!(matches!(first, crate::types::AgentNotice::Stopped { .. }));
    // The instance is already refused, and another id is unknown: both are skipped.
    fx.pc
        .apply_trust_changes(&TrustChanges {
            lost: vec![agent.instance.id, AgentInstanceId::new()],
            gained: vec![AgentInstanceId::new()],
        })
        .await
        .expect("skipped, not failed");
    assert!(
        notices.try_recv().is_err(),
        "a skipped instance raises no notice"
    );
}

#[tokio::test]
async fn install_record_unreadable_after_install() {
    let fx = Fx::new().await;
    *lock(&fx.installers.record_error) = Some(InstallError::RecordInvalid);
    assert_error(
        &fx.pc.add(fx.input()).await.unwrap_err(),
        "PKG_INSTALL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
}

#[tokio::test]
async fn stored_pending_choices_value_unreadable() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    sqlx::query("UPDATE agent_instances SET pending_choices = 'not json'")
        .execute(&fx.pool)
        .await
        .expect("corrupt the column");
    assert_error(
        &fx.pc.pending_choices(agent.instance.id).await.unwrap_err(),
        "PKG_STORE_FAILED",
        false,
        None,
    );
}

#[tokio::test]
async fn datastore_failure() {
    let fx = Fx::new().await;
    fx.pool.close().await;
    assert_error(
        &fx.pc.list(None).await.unwrap_err(),
        "PKG_STORE_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
    // A datastore that is not SQLite is the same code, permanent.
    let fx = Fx::new().await;
    let without_pool = crate::PackageCatalogue::from_parts(
        crate::ports::Ports {
            catalogue: fx.catalogue.clone(),
            resolver: fx.resolver.clone(),
            checker: fx.checker.clone(),
            installers: fx.installers.clone(),
            folders: fx.folders.clone(),
            inspector: fx.inspector.clone(),
        },
        None,
        std::sync::Arc::clone(&fx.journal),
        std::sync::Arc::new(FakeHarness(std::sync::Mutex::new(
            ordin_types::domain::HarnessKind::ClaudeCode,
        ))),
        fx.activity.clone(),
    );
    assert_error(
        &without_pool.list(None).await.unwrap_err(),
        "PKG_STORE_FAILED",
        false,
        None,
    );
}
