//! X2 for the amendment *The document agent comes installed* (PKG-DOC-1 to PKG-DOC-3): one test per
//! scenario, with the scenario's title in the comment above it, and one per Error Table row.

use ordin_catalogue::PartStateLabel;
use ordin_checker::InstallVerdict;
use ordin_resolver::model::{PartKind, BUNDLED_REPOSITORY_ID};
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::{AgentInstanceId, PartId, WorkspaceId};

use super::support::*;
use crate::types::{AddAgentInput, AddOutcome, RemoveAgentInput};
use crate::STARTER_AGENTS;

const DOCUMENT_AGENT: &str = "urn:ordin:default:workspaces:document-agent";

/// The fixture with the document agent in the bundled catalogue, and its part id.
async fn with_document_agent() -> (Fx, PartId) {
    let fx = Fx::new().await;
    let id = PartId::new();
    lock(&fx.catalogue.parts).push(view(
        part(
            id,
            BUNDLED_REPOSITORY_ID,
            PartKind::WorkspaceProfile,
            DOCUMENT_AGENT,
            "Document agent",
            "0.3.0",
        ),
        true,
        PartStateLabel::Current,
    ));
    (fx, id)
}

async fn starters(fx: &Fx, workspace: WorkspaceId) -> Vec<AgentInstanceId> {
    fx.pc
        .add_starter_agents(workspace, Vec::new(), "en-GB")
        .await
        .expect("add_starter_agents")
}

/// The live document agents serving `workspace`.
async fn document_agents_in(fx: &Fx, workspace: WorkspaceId) -> Vec<AgentInstanceId> {
    let mut out = Vec::new();
    for agent in fx.pc.list(Some(workspace)).await.expect("list") {
        let identifier = fx.column(agent.instance.id, "agent_identifier").await;
        if agent.instance.status != AgentInstanceStatus::Removed
            && identifier.as_deref() == Some(DOCUMENT_AGENT)
        {
            out.push(agent.instance.id);
        }
    }
    out
}

async fn offers(fx: &Fx, workspace: WorkspaceId) -> usize {
    use sqlx::Row;
    let row = sqlx::query("SELECT COUNT(*) FROM starter_agent_offers WHERE workspace_id = ?1")
        .bind(workspace.to_string())
        .fetch_one(&fx.pool)
        .await
        .expect("count");
    usize::try_from(row.get::<i64, _>(0)).expect("a count")
}

// PKG-DOC-1: the document agent is the one starter agent.
#[test]
fn the_document_agent_is_the_starter_agent() {
    assert_eq!(STARTER_AGENTS, &[DOCUMENT_AGENT]);
}

// Scenario: Happy path — a new workspace gets the document agent (PKG-DOC-2)
#[tokio::test]
async fn happy_path_a_new_workspace_gets_the_document_agent() {
    let (fx, _) = with_document_agent().await;
    let added = starters(&fx, fx.workspace).await;
    assert_eq!(added.len(), 1);
    let agent = fx.pc.get(added[0]).await.expect("installed");
    assert_eq!(agent.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(agent.workspaces, vec![fx.workspace]);
    assert_eq!(
        agent.instance.agent_repository_id,
        Some(BUNDLED_REPOSITORY_ID)
    );
    assert_eq!(fx.journal_kinds("agent_added").await, 1);
    assert_eq!(offers(&fx, fx.workspace).await, 1);
    // And a second call for W adds nothing.
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert_eq!(document_agents_in(&fx, fx.workspace).await, added);
    assert_eq!(fx.journal_kinds("agent_added").await, 1);
}

// DSK-DOC-1's upgrade case at the catalogue: each workspace is offered the agent once.
#[tokio::test]
async fn each_workspace_gets_the_document_agent() {
    let (fx, _) = with_document_agent().await;
    let other = WorkspaceId::new();
    assert_eq!(starters(&fx, fx.workspace).await.len(), 1);
    assert_eq!(starters(&fx, other).await.len(), 1);
    assert_eq!(document_agents_in(&fx, fx.workspace).await.len(), 1);
    assert_eq!(document_agents_in(&fx, other).await.len(), 1);
}

// Scenario: Edge case — a removed starter agent stays removed (PKG-DOC-3)
#[tokio::test]
async fn edge_case_a_removed_starter_agent_stays_removed() {
    let (fx, _) = with_document_agent().await;
    let added = starters(&fx, fx.workspace).await;
    fx.pc
        .remove(RemoveAgentInput {
            agent_instance_id: added[0],
            workspace_id: Some(fx.workspace),
        })
        .await
        .expect("removed");
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert!(document_agents_in(&fx, fx.workspace).await.is_empty());
}

// Scenario: Edge case — the person added it first (PKG-DOC-2)
#[tokio::test]
async fn edge_case_the_person_added_it_first() {
    let (fx, id) = with_document_agent().await;
    let first = match fx
        .pc
        .add(AddAgentInput {
            workspace_id: fx.workspace,
            agent: id,
            workspace_languages: Vec::new(),
            interface_locale: "en-GB".to_owned(),
            choices: Vec::new(),
        })
        .await
        .expect("the person's add")
    {
        AddOutcome::Ready { agent } => agent.instance.id,
        other => panic!("expected ready, got {other:?}"),
    };
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert_eq!(document_agents_in(&fx, fx.workspace).await, vec![first]);
    assert_eq!(offers(&fx, fx.workspace).await, 1, "the offer is recorded");
}

// Scenario: Error path — a starter agent missing from the bundled catalogue (PKG-DOC-2)
// Error Table: a starter agent is not in the bundled catalogue → skipped, nothing shown.
#[tokio::test]
async fn error_path_a_starter_agent_missing_from_the_bundled_catalogue() {
    let fx = Fx::new().await;
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert_eq!(fx.instance_count().await, 0);
    assert_eq!(
        offers(&fx, fx.workspace).await,
        0,
        "no row: a later version that ships it offers it"
    );
}

// PKG-DOC-2 step 2: a part that is not Current counts as missing.
#[tokio::test]
async fn a_starter_agent_that_is_not_current_is_skipped() {
    let (fx, _) = with_document_agent().await;
    for v in lock(&fx.catalogue.parts).iter_mut() {
        if v.part.identifier == DOCUMENT_AGENT {
            v.state_label = PartStateLabel::NotForNewAgents;
        }
    }
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert_eq!(fx.instance_count().await, 0);
    assert_eq!(offers(&fx, fx.workspace).await, 0);
}

// PKG-DOC-2 step 2: another catalogue failure is returned with its flag and records nothing.
#[tokio::test]
async fn a_library_failure_is_returned_and_records_nothing() {
    let (fx, _) = with_document_agent().await;
    *lock(&fx.catalogue.find_error) = Some(("CAT_STORE_FAILED", true));
    let error = fx
        .pc
        .add_starter_agents(fx.workspace, Vec::new(), "en-GB")
        .await
        .unwrap_err();
    assert_eq!(error.code, "PKG_LIBRARY_UNAVAILABLE");
    assert!(error.retryable);
    assert_eq!(offers(&fx, fx.workspace).await, 0);
}

// Error Table: a starter agent's add is refused (an outcome) → skipped, the offer recorded.
#[tokio::test]
async fn error_a_refused_add_is_recorded_and_not_tried_again() {
    let (fx, _) = with_document_agent().await;
    *lock(&fx.checker.verdict) = InstallVerdict::Failed;
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert_eq!(offers(&fx, fx.workspace).await, 1);
    *lock(&fx.checker.verdict) = InstallVerdict::Clean;
    assert!(
        starters(&fx, fx.workspace).await.is_empty(),
        "the library still offers it; the start does not"
    );
}

// PKG-DOC-2 step 3: a retryable add failure is returned with no row, and the next call adds it.
#[tokio::test]
async fn a_retryable_add_failure_is_tried_again_at_the_next_call() {
    let (fx, _) = with_document_agent().await;
    *lock(&fx.catalogue.scope_error) = Some(("CAT_STORE_FAILED", true));
    let error = fx
        .pc
        .add_starter_agents(fx.workspace, Vec::new(), "en-GB")
        .await
        .unwrap_err();
    assert!(error.retryable);
    assert_eq!(offers(&fx, fx.workspace).await, 0);
    *lock(&fx.catalogue.scope_error) = None;
    assert_eq!(starters(&fx, fx.workspace).await.len(), 1);
}

// PKG-DOC-2 step 3: a permanent add failure is logged and recorded, never tried again.
#[tokio::test]
async fn a_permanent_add_failure_is_recorded() {
    let (fx, _) = with_document_agent().await;
    *lock(&fx.catalogue.scope_error) = Some(("CAT_REPOSITORY_UNKNOWN", false));
    assert!(starters(&fx, fx.workspace).await.is_empty());
    assert_eq!(offers(&fx, fx.workspace).await, 1);
}
