//! Behavioural check (plan 01, Step 53): a knowledge page given to the team is pushed to a local
//! bare Git repository by the Push Service as built, completes, and is journalled as exactly one
//! external action whose key is the one the Push Service stored before the send.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "support/push_stack.rs"]
mod push_stack;
mod support;

use ordin_knowledge::{KnowledgeService, ProgramKind, WritePage};
use ordin_push::{CardText, PushNotice, TeamShareProposal};
use ordin_types::domain::places::KnowledgeAuthor;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::{PlaceId, RunId, TeamShareId};
use support::fakes::Fakes;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_knowledge_page_pushed_to_a_local_bare_repository_is_journalled_as_one_external_action_with_its_key(
) {
    let stack = push_stack::Stack::new(KnowledgeService::migrations()).await;
    let fakes = Fakes::new();
    let place = PlaceId::new();
    fakes.files.add_place(place);
    let w = fakes.workspaces.add("Q3 supplier review", place);
    let knowledge = KnowledgeService::new(
        ProgramKind::Desktop,
        fakes.deps(stack.journal.clone(), None),
    )
    .await
    .unwrap();
    let page = knowledge
        .write_page(
            KnowledgeAuthor::Ordin,
            WritePage {
                workspace_id: w,
                page_id: None,
                title: "How supplier rates are set".into(),
                body_markdown: "## Review\nRates are reviewed each quarter.".into(),
                sources: vec![],
                run_id: Some(RunId::new()),
                tags: None,
                aliases: None,
                page_type: None,
                description: None,
            },
        )
        .await
        .unwrap();
    // The Team Knowledge Service's part: the exported file, named for the sharer, committed.
    let exported = knowledge.export_team_pages(w).await.unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(exported[0].page_id, page);
    let text = String::from_utf8(exported[0].bytes.clone())
        .unwrap()
        .replace("shared_by: \"\"", "shared_by: \"Asha Patel\"");
    let remote = push_stack::bare_with_main(&stack.dir.0, "team-remote");
    let clone = push_stack::bare_clone_of(&remote, &stack.dir.0, "team-clone");
    let file = format!("{page}.md");
    let commit = push_stack::commit_page(&clone, &file, text.as_bytes());

    let card = stack
        .service
        .propose_team_share(TeamShareProposal {
            share_id: TeamShareId::new(),
            workspace_id: w,
            clone_path: clone.clone(),
            commit: commit.to_string(),
            host: "github.com".into(),
            repository: "acme/supplier-team".into(),
            branch: "main".into(),
            review_branch: None,
            title: "Team knowledge from Asha Patel: 1 page".into(),
            body: "Shared from ordin after approval".into(),
            card: CardText {
                question: "Share what you've learned with acme/supplier-team?".into(),
                verb: "Share".into(),
                where_line: "acme/supplier-team › knowledge".into(),
                affects: vec![AffectedItem::Record {
                    label: "1 page".into(),
                }],
                changes: "1 page. What you share stays in the repository's history.".into(),
                content: None,
            },
        })
        .await
        .expect("propose the share");
    stack.approve(card).await;
    stack
        .notice(|n| matches!(n, PushNotice::TeamShareSent { .. }).then_some(()))
        .await;

    assert_eq!(push_stack::main_of(&remote), commit);
    assert_eq!(
        push_stack::file_at_main(&remote, &format!("pages/{file}")),
        text.as_bytes()
    );
    let key: String = sqlx::query_scalar("SELECT action_key FROM team_share_submissions")
        .fetch_one(&stack.pool)
        .await
        .expect("the key stored on the submission");
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT action_key, kind, state FROM external_actions")
            .fetch_all(&stack.pool)
            .await
            .expect("actions");
    assert_eq!(
        rows,
        vec![(key, "origin_push".to_owned(), "confirmed".to_owned())]
    );
    stack.shutdown.cancel();
}
