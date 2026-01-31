//! X2 for the amendment *Bundled agents* (PKG-REC-1 to PKG-REC-4): one test per scenario, with the
//! scenario's title in the comment above it, and one per Error Table row.

use ordin_catalogue::PartStateLabel;
use ordin_checker::InstallVerdict;
use ordin_resolver::model::{PartKind, BUNDLED_REPOSITORY_ID};
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::PartId;

use super::support::*;
use crate::types::{AgentNotice, BundledAgent, ChangeChoiceInput, PackChoice, RemoveAgentInput};

const ADVISOR: &str = "urn:ordin:default:workspaces:advisor";

/// The fixture with the agent maker offered in the bundled catalogue.
async fn with_agent_maker() -> Fx {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts).push(view(
        part(
            PartId::new(),
            BUNDLED_REPOSITORY_ID,
            PartKind::WorkspaceProfile,
            ADVISOR,
            "Agent maker",
            "1.2.0",
        ),
        true,
        PartStateLabel::Current,
    ));
    fx
}

async fn status(fx: &Fx) -> Option<String> {
    use sqlx::Row;
    sqlx::query("SELECT status FROM agent_instances WHERE id = ?1")
        .bind(BundledAgent::AgentMaker.instance_id().to_string())
        .fetch_optional(&fx.pool)
        .await
        .expect("select")
        .map(|r| r.get::<String, _>(0))
}

// PKG-REC-1: the fixed ids, the identifiers and the log names.
#[test]
fn the_bundled_agents_have_fixed_ids_and_identifiers() {
    assert_eq!(
        BundledAgent::Ordin.instance_id().to_string(),
        "01900000-0000-7000-8000-000000000101"
    );
    assert_eq!(
        BundledAgent::AgentMaker.instance_id().to_string(),
        "01900000-0000-7000-8000-000000000102"
    );
    assert_eq!(BundledAgent::AgentMaker.identifier(), Some(ADVISOR));
    assert_eq!(
        BundledAgent::Ordin.identifier(),
        Some("urn:ordin:default:workspaces:coordinator")
    );
    assert_eq!(BundledAgent::Ordin.log_name(), "ordin");
    assert_eq!(BundledAgent::AgentMaker.log_name(), "agent_maker");
}

// Scenario: Happy path — the agent maker is added on first use (PKG-REC-2)
#[tokio::test]
async fn happy_path_the_agent_maker_is_added_on_first_use() {
    let fx = with_agent_maker().await;
    let mut notices = fx.pc.subscribe();
    let agent = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    assert_eq!(agent.instance.id, BundledAgent::AgentMaker.instance_id());
    assert_eq!(agent.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(
        agent.instance.agent_repository_id,
        Some(BUNDLED_REPOSITORY_ID)
    );
    assert!(agent.workspaces.is_empty(), "no workspace");
    assert_eq!(fx.journal_kinds("agent_added").await, 1);
    let check = lock(&fx.checker.checks).last().cloned().expect("a check");
    assert_eq!(
        check.agent_instance_id,
        Some(BundledAgent::AgentMaker.instance_id())
    );
    assert!(
        !matches!(notices.try_recv(), Ok(AgentNotice::Added { .. })),
        "the Added notice names a workspace and is not sent"
    );
    assert_eq!(
        fx.column(agent.instance.id, "interface_locale")
            .await
            .as_deref(),
        Some("en-GB")
    );
}

// Scenario: Edge case — a second call writes nothing (PKG-REC-2)
#[tokio::test]
async fn edge_case_a_second_call_writes_nothing() {
    let fx = with_agent_maker().await;
    fx.pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    fx.clear_log();
    let again = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "fr-FR")
        .await
        .expect("the stored instance");
    assert_eq!(again.instance.status, AgentInstanceStatus::Ready);
    assert!(fx.log_lines().is_empty(), "{:?}", fx.log_lines());
    assert_eq!(fx.journal_kinds("agent_added").await, 1);
}

// Scenario: Error path — Ordin's entry is not in the bundled catalogue (PKG-REC-1, PKG-REC-2)
#[tokio::test]
async fn error_path_ordins_entry_is_not_in_the_bundled_catalogue() {
    let fx = with_agent_maker().await;
    let err = fx
        .pc
        .bundled_instance(BundledAgent::Ordin, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_MISSING");
    assert!(!err.retryable);
    assert_eq!(fx.instance_count().await, 0);
}

// Error Table: the bundled catalogue holds no current, offered WorkspaceProfile with the identifier.
#[tokio::test]
async fn error_the_bundled_catalogue_lacks_the_agent() {
    let fx = Fx::new().await;
    let err = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_MISSING");
    // A version that is not current is missing too.
    lock(&fx.catalogue.parts).push(view(
        part(
            PartId::new(),
            BUNDLED_REPOSITORY_ID,
            PartKind::WorkspaceProfile,
            ADVISOR,
            "Agent maker",
            "1.2.0",
        ),
        true,
        PartStateLabel::Withdrawn,
    ));
    let err = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_MISSING");
    // Another catalogue failure keeps its own retryable flag.
    *lock(&fx.catalogue.find_error) = Some(("CAT_DATASTORE_UNAVAILABLE", true));
    let err = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_LIBRARY_UNAVAILABLE");
    assert!(err.retryable);
    assert_eq!(fx.instance_count().await, 0);
}

// Error Table: the bundled agent fails its checks or needs IT approval.
#[tokio::test]
async fn error_the_bundled_agent_fails_its_checks() {
    let fx = with_agent_maker().await;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    let err = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_REFUSED");
    assert_eq!(err.action, None);
    assert_eq!(fx.journal_kinds("agent_add_refused").await, 1);
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("runner")],
    };
    let err = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_REFUSED");
    assert!(err.detail.contains("runner"), "{}", err.detail);
    assert_eq!(fx.instance_count().await, 0);
}

// Scenario: Edge case — an interrupted install is cleaned and repeated (PKG-REC-2)
#[tokio::test]
async fn edge_case_an_interrupted_install_is_cleaned_and_repeated() {
    let fx = with_agent_maker().await;
    fx.pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    sqlx::query("UPDATE agent_instances SET status = 'installing' WHERE id = ?1")
        .bind(BundledAgent::AgentMaker.instance_id().to_string())
        .execute(&fx.pool)
        .await
        .expect("interrupt");
    fx.clear_log();
    let agent = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("installed again");
    assert_eq!(agent.instance.id, BundledAgent::AgentMaker.instance_id());
    assert_eq!(status(&fx).await.as_deref(), Some("ready"));
    let lines = fx.log_lines();
    assert_eq!(count(&lines, "installers.uninstall"), 1, "{lines:?}");
    assert!(count(&lines, "folders.remove") >= 1, "{lines:?}");
    assert_eq!(count(&lines, "folders.create"), 1, "{lines:?}");
    assert_eq!(fx.journal_kinds("agent_added").await, 2);
}

// PKG-REC-2 step 2: a bundled instance stopped by a withdrawal or lost trust is refused as it is.
#[tokio::test]
async fn error_a_stopped_bundled_agent_is_refused() {
    let fx = with_agent_maker().await;
    fx.pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    sqlx::query(
        "UPDATE agent_instances SET status = 'refused', stopped_reason = 'withdrawn by the company', stopped_by = 'withdrawal' WHERE id = ?1",
    )
    .bind(BundledAgent::AgentMaker.instance_id().to_string())
    .execute(&fx.pool)
    .await
    .expect("stop");
    fx.clear_log();
    let err = fx
        .pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_REFUSED");
    assert_eq!(err.detail, "withdrawn by the company");
    assert!(fx.log_lines().is_empty(), "nothing is written");
}

// Scenario: Error path — a bundled agent cannot be removed (PKG-REC-3)
#[tokio::test]
async fn error_path_a_bundled_agent_cannot_be_removed() {
    let fx = with_agent_maker().await;
    let err = fx
        .pc
        .remove(RemoveAgentInput {
            agent_instance_id: BundledAgent::Ordin.instance_id(),
            workspace_id: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_FIXED");
    let err = fx
        .pc
        .change_choice(ChangeChoiceInput {
            agent_instance_id: BundledAgent::AgentMaker.instance_id(),
            choice: PackChoice {
                rule_index: 0,
                packs: vec![],
            },
        })
        .await
        .unwrap_err();
    assert_eq!(err.code, "PKG_BUNDLED_AGENT_FIXED");
    assert_eq!(
        err.detail,
        "This agent comes with ordin and can't be changed or removed."
    );
}

// Scenario: Edge case — bundled agents are not in the agent list (PKG-REC-3)
#[tokio::test]
async fn edge_case_bundled_agents_are_not_in_the_agent_list() {
    let fx = with_agent_maker().await;
    fx.pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    let added = fx.add().await;
    let listed = fx.pc.list(None).await.expect("listed");
    let ids: Vec<_> = listed.iter().map(|a| a.instance.id).collect();
    assert_eq!(ids, vec![added.instance.id]);
    assert!(fx
        .pc
        .get(BundledAgent::AgentMaker.instance_id())
        .await
        .is_ok());
    let in_workspace = fx.pc.list(Some(fx.workspace)).await.expect("listed");
    assert_eq!(in_workspace.len(), 1);
}

// Scenario: Happy path — a new application version updates a bundled agent without asking (PKG-REC-3)
#[tokio::test]
async fn happy_path_a_new_application_version_updates_a_bundled_agent_without_asking() {
    let fx = with_agent_maker().await;
    let id = BundledAgent::AgentMaker.instance_id();
    fx.pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    *lock(&fx.inspector.candidate_may_do) = may_do(&["files", "mail"]);
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc
        .on_repository_updated(BUNDLED_REPOSITORY_ID, &[], &shutdown())
        .await;
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.updated_between_runs_at.is_some())
    })
    .await;
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(updated.instance.status, AgentInstanceStatus::Ready);
    assert!(updated.update.is_none(), "no update waits for the person");
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
}

// Scenario: Happy path — an agent installed at an old version is updated to the new one (DSK-PKG-1)
// The document agent was installed from the bundled repository at 0.1.0; the bundled catalogue an
// upgrade indexed holds 0.2.0, which changes nothing it may do or read. `run`'s start sweep, with no
// notice sent, updates it between runs.
// spec: ordin-desktop/components/OrdinDesktop#DSK-PKG-1
// spec: ordin-desktop/components/PackageCatalogue#PKG-REC-3
#[tokio::test]
async fn happy_path_the_start_sweep_updates_an_agent_of_the_bundled_repository() {
    let fx = Fx::new().await;
    let document_agent = PartId::new();
    lock(&fx.catalogue.parts).push(view(
        part(
            document_agent,
            BUNDLED_REPOSITORY_ID,
            PartKind::WorkspaceProfile,
            "urn:ordin:default:workspaces:document-agent",
            "Document agent",
            "0.1.0",
        ),
        true,
        PartStateLabel::Current,
    ));
    fx.set_plan(plan("a1", COMMIT_A, "0.1.0"));
    let installed = fx.add_agent(document_agent).await;
    let id = installed.instance.id;
    assert_eq!(installed.instance.package_version, "0.1.0");
    assert_eq!(
        installed.instance.agent_repository_id,
        Some(BUNDLED_REPOSITORY_ID)
    );
    fx.set_plan(plan("b1", COMMIT_B, "0.2.0"));

    let stop = shutdown();
    let pc = fx.pc.clone();
    let token = stop.clone();
    let running = tokio::spawn(async move { pc.run(token).await });
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.package_version == "0.2.0")
    })
    .await;
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(updated.instance.status, AgentInstanceStatus::Ready);
    assert!(updated.instance.updated_between_runs_at.is_some());
    assert!(updated.update.is_none(), "no update waits for the person");
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
    stop.cancel();
    running
        .await
        .expect("the loop ends")
        .expect("the loop ends without an error");
}

// PKG-REC-3: a bundled candidate that needs IT approval keeps the installed version.
#[tokio::test]
async fn a_bundled_candidate_that_needs_it_approval_keeps_the_installed_version() {
    let fx = with_agent_maker().await;
    let id = BundledAgent::AgentMaker.instance_id();
    fx.pc
        .bundled_instance(BundledAgent::AgentMaker, "en-GB")
        .await
        .expect("added");
    *lock(&fx.checker.verdict) = InstallVerdict::NeedsItApproval {
        parts: vec![exec_part("runner")],
    };
    fx.set_plan(plan("b1", COMMIT_B, "1.3.0"));
    fx.pc
        .on_repository_updated(BUNDLED_REPOSITORY_ID, &[], &shutdown())
        .await;
    let kept = fx.pc.get(id).await.expect("the instance");
    assert_eq!(kept.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(kept.instance.package_version, "1.2.0");
    assert_eq!(fx.journal_kinds("agent_update_refused").await, 1);
}

// PKG-OWN-1: Ordin and the agent maker are ordin's own agents and are never offered to add; the same
// identifiers in another repository are ordinary agents.
// spec: ordin-desktop/components/PackageCatalogue#PKG-OWN-1
#[test]
fn ordins_own_agents_are_told_apart_by_repository_and_identifier() {
    use crate::types::is_own_agent;
    use ordin_resolver::model::BUNDLED_REPOSITORY_ID;
    assert!(is_own_agent(
        BUNDLED_REPOSITORY_ID,
        "urn:ordin:default:workspaces:coordinator"
    ));
    assert!(is_own_agent(
        BUNDLED_REPOSITORY_ID,
        "urn:ordin:default:workspaces:advisor"
    ));
    assert!(!is_own_agent(
        BUNDLED_REPOSITORY_ID,
        "urn:ordin:default:workspaces:document-agent"
    ));
    assert!(!is_own_agent(
        ordin_types::AgentRepositoryId::new(),
        "urn:ordin:default:workspaces:coordinator"
    ));
}
