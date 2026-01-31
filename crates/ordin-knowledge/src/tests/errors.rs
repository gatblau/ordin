//! X1: one test per Error Table row of the desktop and personal-server half (base table and the
//! rows each amendment adds), asserting the code, the retryability and the action where stated.
//! The agent-profile rows are in `tests/agent_profile.rs`. X4: every owned code is registered.

use ordin_files::PlaceChangeKind;
use ordin_mcp_front::ToolCaller;
use ordin_types::domain::places::{
    CopyDestination, KnowledgeAuthor, KnowledgeSource, WorkspaceState,
};
use ordin_types::error_registry::{lookup, owns_prefix};
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, CodedError, KnowledgePageId, KnowledgeSuggestionId, RepairAction, RunId,
    TeamFolderId, WorkspaceId,
};
use serde_json::json;

use super::fakes::{sqlite_journal, Fakes, TempDir};
use super::rig::{src_kind, Rig};
use super::team::team_bytes;
use crate::error::{KnowledgeError, ALL_CODES};
use crate::model::{AdoptEdit, NotAdoptedReason, ProgramKind, TeamPageImport};
use crate::service::KnowledgeService;

fn assert_code<T: std::fmt::Debug>(
    r: Result<T, KnowledgeError>,
    code: &str,
    retryable: bool,
) -> KnowledgeError {
    let e = r.unwrap_err();
    assert_eq!(e.code(), code);
    assert_eq!(e.retryable(), retryable, "{code}");
    assert_eq!(e.to_string(), code);
    e
}

#[test]
fn every_owned_code_is_registered_and_the_prefix_is_owned() {
    assert_eq!(ALL_CODES.len(), 50);
    for code in ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not in the registry"));
        assert_eq!(entry.owner_crate, "ordin-knowledge", "{code}");
        assert!(
            owns_prefix("ordin-knowledge", code.split('_').next().unwrap()),
            "{code}"
        );
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, ALL_CODES);
}

#[test]
fn the_registry_retry_flag_of_an_always_retryable_code_matches_retryable() {
    // FU-102: the registry and `KnowledgeError::retryable` said different things for these two.
    // `KNOW_ADOPT_REFUSED` carries its cause's retryability (ADR-110), so one flag cannot state it.
    for error in [
        KnowledgeError::DbUnavailable,
        KnowledgeError::HistoryUnavailable,
    ] {
        let entry = lookup(error.code()).expect("registered");
        assert_eq!(entry.retryable, error.retryable(), "{}", error.code());
    }
}

// ---------------------------------------------------------------- base table

#[tokio::test]
async fn row_title_empty_over_200_characters_or_with_control_characters() {
    let rig = Rig::desktop().await;
    for title in ["  ".to_owned(), "x".repeat(201), "a\u{7}b".to_owned()] {
        let e = assert_code(
            rig.svc
                .write_page(KnowledgeAuthor::Person, rig.req(&title, "x", vec![]))
                .await,
            "KNOW_TITLE_INVALID",
            false,
        );
        assert_eq!(e.repair(), None);
    }
}

#[tokio::test]
async fn row_title_already_used_by_another_page_of_the_workspace() {
    let rig = Rig::desktop().await;
    rig.write("Rates", "x").await;
    assert_code(
        rig.svc
            .write_page(KnowledgeAuthor::Person, rig.req("RATES", "y", vec![]))
            .await,
        "KNOW_TITLE_DUPLICATE",
        false,
    );
}

#[tokio::test]
async fn row_body_over_1_mib() {
    let rig = Rig::desktop().await;
    let body = "x".repeat(1_048_577);
    assert_code(
        rig.svc
            .write_page(KnowledgeAuthor::Ordin, rig.req("Big", &body, vec![]))
            .await,
        "KNOW_BODY_TOO_LARGE",
        false,
    );
}

#[tokio::test]
async fn row_more_than_100_sources() {
    let rig = Rig::desktop().await;
    let sources: Vec<KnowledgeSource> = (0..101)
        .map(|i| rig.place_src(&format!("f{i}.txt"), "1"))
        .collect();
    assert_code(
        rig.svc
            .write_page(KnowledgeAuthor::Ordin, rig.req("Many", "x", sources))
            .await,
        "KNOW_SOURCES_TOO_MANY",
        false,
    );
}

#[tokio::test]
async fn row_source_origin_not_place_sharepoint_confluence_or_location_version_out_of_bounds() {
    let rig = Rig::desktop().await;
    for s in [
        src_kind("email", "x", "1"),
        src_kind("sharepoint", "", "1"),
        src_kind("sharepoint", &"l".repeat(2049), "1"),
        src_kind("confluence", "page", ""),
        src_kind("confluence", "page", &"v".repeat(257)),
    ] {
        assert_code(
            rig.svc
                .write_page(KnowledgeAuthor::Ordin, rig.req("S", "x", vec![s]))
                .await,
            "KNOW_SOURCE_INVALID",
            false,
        );
    }
}

#[tokio::test]
async fn row_500_pages_already_in_the_workspace() {
    let rig = Rig::desktop().await;
    rig.exec(&format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500) \
         INSERT INTO knowledge_pages (id, workspace_id, title, body_markdown, written_by, state, last_changed_at, place_id, \
         relative_path, file_version, created_at) SELECT '0192ffff-0000-7000-8000-' || printf('%012d', i), '{}', 'T' || i, 'x', 'ordin', 'current', \
         '2026-01-01T00:00:00.000Z', '{}', 'knowledge/' || i || '.md', 1, '2026-01-01T00:00:00.000Z' FROM n",
        rig.w, rig.place
    ))
    .await;
    assert_code(
        rig.svc
            .write_page(KnowledgeAuthor::Ordin, rig.req("One more", "x", vec![]))
            .await,
        "KNOW_PAGE_LIMIT",
        false,
    );
}

#[tokio::test]
async fn row_page_id_unknown_or_bin_emptied_before_undo() {
    let rig = Rig::desktop().await;
    assert_code(
        rig.svc.get_page(KnowledgePageId::new()).await,
        "KNOW_PAGE_NOT_FOUND",
        false,
    );
    let id = rig.write("Rates", "x").await;
    rig.svc.remove_page(id).await.unwrap();
    let ticket = rig.row(id).await.bin_ticket.unwrap();
    // The system's bin was emptied, and the version copy is gone with it.
    rig.f.files.external_remove(rig.place, "knowledge/Rates.md");
    rig.exec("UPDATE knowledge_pages SET bin_ticket = json_set(bin_ticket, '$.file_version_id', '0192ffff-0000-7000-8000-000000000009')").await;
    let _ = ticket;
    assert_code(rig.svc.undo_remove(id).await, "KNOW_PAGE_NOT_FOUND", false);
    let mut conn = rig.svc.conn().await.unwrap();
    assert!(crate::store::page(&mut conn, id).await.unwrap().is_none());
}

#[tokio::test]
async fn row_write_or_remove_on_a_removed_page() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "x").await;
    rig.svc.remove_page(id).await.unwrap();
    assert_code(rig.svc.remove_page(id).await, "KNOW_PAGE_REMOVED", false);
    let mut req = rig.req("Rates", "y", vec![]);
    req.page_id = Some(id);
    assert_code(
        rig.svc.write_page(KnowledgeAuthor::Ordin, req).await,
        "KNOW_PAGE_REMOVED",
        false,
    );
}

#[tokio::test]
async fn row_undo_on_a_page_that_is_not_removed() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "x").await;
    assert_code(rig.svc.undo_remove(id).await, "KNOW_NOT_REMOVED", false);
}

#[tokio::test]
async fn row_workspace_absent_or_in_ordins_bin() {
    let rig = Rig::desktop().await;
    let mut req = rig.req("Rates", "x", vec![]);
    req.workspace_id = WorkspaceId::new();
    assert_code(
        rig.svc.write_page(KnowledgeAuthor::Ordin, req).await,
        "KNOW_WORKSPACE_NOT_FOUND",
        false,
    );
    rig.f.workspaces.set_state(rig.w, WorkspaceState::InBin);
    assert_code(
        rig.svc
            .write_page(KnowledgeAuthor::Ordin, rig.req("Rates", "x", vec![]))
            .await,
        "KNOW_WORKSPACE_NOT_FOUND",
        false,
    );
}

async fn out_of_date(rig: &Rig) -> KnowledgePageId {
    let id = rig
        .svc
        .write_page(
            KnowledgeAuthor::Ordin,
            rig.req("Rates", "x", vec![rig.place_src("a.txt", "1")]),
        )
        .await
        .unwrap();
    rig.report("a.txt", PlaceChangeKind::Modified, Some(2))
        .await;
    id
}

#[tokio::test]
async fn row_use_or_keep_an_update_when_none_is_pending() {
    let rig = Rig::desktop().await;
    let id = out_of_date(&rig).await;
    assert_code(
        rig.svc.use_update(id).await,
        "KNOW_NO_PENDING_UPDATE",
        false,
    );
    assert_code(rig.svc.keep_page(id).await, "KNOW_NO_PENDING_UPDATE", false);
}

#[tokio::test]
async fn row_leave_it_or_propose_update_on_a_current_page() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "x").await;
    assert_code(rig.svc.leave_it(id).await, "KNOW_NOT_OUT_OF_DATE", false);
    assert_code(
        rig.svc
            .propose_update(id, "y".into(), vec![], RunId::new())
            .await,
        "KNOW_NOT_OUT_OF_DATE",
        false,
    );
}

#[tokio::test]
async fn row_file_service_cannot_write_or_read_the_page_file() {
    let rig = Rig::desktop().await;
    rig.f.files.fail_writes(true);
    let e = assert_code(
        rig.svc
            .write_page(KnowledgeAuthor::Ordin, rig.req("Rates", "x", vec![]))
            .await,
        "KNOW_FILE_UNAVAILABLE",
        true,
    );
    assert_eq!(e.repair(), Some(RepairAction::TryAgain));
}

#[tokio::test]
async fn row_page_file_header_cannot_be_read() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "x").await;
    rig.edit_outside("knowledge/Rates.md", "---\ntitle: [broken\n---\nThe body.")
        .await;
    let journal = rig.journal().await;
    let entry = journal
        .iter()
        .find(|(k, _)| k == "knowledge_header_invalid")
        .unwrap();
    assert_eq!(entry.1, json!({ "page_id": id }));
    let row = rig.row(id).await;
    assert_eq!(row.title, "Rates");
    assert!(row.body.contains("The body."));
}

#[tokio::test]
async fn row_agent_reference_page_not_in_the_agents_knowledge_folder() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    rig.f.workspaces.agents.lock().unwrap().insert(
        rig.w,
        vec![crate::ports::WorkspaceAgent {
            agent_instance_id: agent,
            display_name: "A".into(),
        }],
    );
    assert_code(
        rig.svc
            .read_agent_page(rig.w, agent, "knowledge/gone.md")
            .await,
        "KNOW_AGENT_PAGE_NOT_FOUND",
        false,
    );
    assert_code(
        rig.svc
            .read_agent_page(rig.w, agent, "../../etc/passwd")
            .await,
        "KNOW_AGENT_PAGE_NOT_FOUND",
        false,
    );
}

#[tokio::test]
async fn row_push_service_refuses_the_publish_request() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "x").await;
    *rig.f.push.fail.lock().unwrap() = true;
    let dest = CopyDestination::ConfluencePage {
        space: "S".into(),
        parent_page_id: "1".into(),
    };
    let e = assert_code(
        rig.svc.publish_to_confluence(id, dest, false).await,
        "KNOW_PUBLISH_FAILED",
        true,
    );
    assert_eq!(e.repair(), Some(RepairAction::TryAgain));
}

#[tokio::test(flavor = "current_thread")]
async fn row_agent_dependencies_present_on_a_desktop_program_or_absent_in_the_agent_profile() {
    let (logs, _guard) = crate::tests::logs::capture();
    let dir = TempDir::new();
    let (_, journal) = sqlite_journal(&dir).await;
    let f = Fakes::new();
    let e = KnowledgeService::new(
        ProgramKind::Desktop,
        f.deps(journal.clone(), Some((vec![], false))),
    )
    .await;
    assert_code(e.map(|_| ()), "KNOW_PROFILE_MISMATCH", false);
    let e = KnowledgeService::new(ProgramKind::AgentScaled, f.deps(journal, None)).await;
    assert_code(e.map(|_| ()), "KNOW_PROFILE_MISMATCH", false);
    assert_eq!(logs.named("knowledge_start_failed").len(), 2);
}

#[tokio::test]
async fn row_knowledge_search_pages_arguments_outside_the_schema() {
    let rig = Rig::desktop().await;
    let provider = rig.svc.tool_provider();
    for args in [
        json!({"query": ""}),
        json!({"query": "x", "limit": 21}),
        json!({"query": "x", "other": 1}),
        json!({"query": "x".repeat(501)}),
    ] {
        let e = provider
            .check_arguments(&Rig::key("knowledge_search_pages"), &args)
            .unwrap_err();
        assert_eq!(e.reason, "Give a query or at least one of state, written_by, tag; query 1 to 500 characters; limit 1 to 20.");
    }
}

#[tokio::test]
async fn row_reader_search_pages_with_bad_workspaces_query_or_limit() {
    let rig = Rig::desktop().await;
    let reader = rig.svc.reader();
    let many: Vec<WorkspaceId> = (0..501).map(|_| WorkspaceId::new()).collect();
    assert_code(
        reader.search_pages(&[], "rate", 10).await,
        "KNOW_QUERY_INVALID",
        false,
    );
    assert_code(
        reader.search_pages(&many, "rate", 10).await,
        "KNOW_QUERY_INVALID",
        false,
    );
    assert_code(
        reader.search_pages(&[rig.w], "", 10).await,
        "KNOW_QUERY_INVALID",
        false,
    );
    assert_code(
        reader.search_pages(&[rig.w], &"x".repeat(501), 10).await,
        "KNOW_QUERY_INVALID",
        false,
    );
    assert_code(
        reader.search_pages(&[rig.w], "rate", 0).await,
        "KNOW_QUERY_INVALID",
        false,
    );
    assert_code(
        reader.search_pages(&[rig.w], "rate", 51).await,
        "KNOW_QUERY_INVALID",
        false,
    );
}

#[tokio::test]
async fn row_knowledge_reader_call_in_the_agent_profile() {
    let (svc, _dir) = super::team::agent_service().await;
    assert_code(
        svc.reader().page_version(KnowledgePageId::new()).await,
        "KNOW_PROFILE_MISMATCH",
        false,
    );
    assert_code(
        svc.reader().out_of_date_since(Timestamp::now()).await,
        "KNOW_PROFILE_MISMATCH",
        false,
    );
}

#[tokio::test]
async fn row_knowledge_read_page_with_a_value_that_is_not_a_uuid() {
    let rig = Rig::desktop().await;
    let e = rig
        .svc
        .tool_provider()
        .check_arguments(
            &Rig::key("knowledge_read_page"),
            &json!({"page_id": "not-a-uuid"}),
        )
        .unwrap_err();
    assert_eq!(e.at, "/page_id");
    assert_eq!(e.reason, "page_id must be a page id a search returned.");
}

#[tokio::test]
async fn row_knowledge_read_page_for_a_missing_removed_other_workspace_or_withheld_page() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    let removed = rig.write("Removed", "x").await;
    rig.svc.remove_page(removed).await.unwrap();
    let (w2, _) = rig.workspace("W2");
    let mut req = rig.req("Elsewhere", "x", vec![]);
    req.workspace_id = w2;
    let other = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let withheld = rig.write("Withheld", "x").await;
    let mut req = rig.req("Withheld", "x", vec![src_kind("sharepoint", "Secret", "1")]);
    req.page_id = Some(withheld);
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    rig.f
        .rules
        .refused
        .lock()
        .unwrap()
        .insert((agent, "Secret".into()));
    for id in [KnowledgePageId::new(), removed, other, withheld] {
        let e = rig
            .call(
                rig.ctx(ToolCaller::Agent, agent),
                "knowledge_read_page",
                json!({"page_id": id}),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code, "KNOW_PAGE_NOT_FOUND");
        assert_eq!(
            e.message,
            "No page with that id is available in this workspace."
        );
        assert!(!e.retryable);
    }
}

#[tokio::test]
async fn row_database_unavailable_during_a_tool_call() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "x").await;
    rig.svc.pool().unwrap().close().await;
    for (tool, args) in [
        ("knowledge_search_pages", json!({"query": "rates"})),
        ("knowledge_read_page", json!({"page_id": id})),
    ] {
        let e = rig
            .call(
                rig.ctx(ToolCaller::Ordin, AgentInstanceId::new()),
                tool,
                args,
            )
            .await
            .unwrap_err();
        assert_eq!(e.code, "KNOW_DB_UNAVAILABLE");
        assert_eq!(
            e.message,
            "The knowledge pages cannot be read at the moment."
        );
        assert!(e.retryable);
    }
}

// ---------------------------------------------------------------- ADR-103

#[tokio::test]
async fn row_team_page_file_too_large_header_id_sharer_field_or_other_workspace() {
    let rig = Rig::desktop().await;
    let p = KnowledgePageId::new();
    let good = |id| team_bytes(id, "Kestrel", "x", "Priya Shah", Timestamp::now(), vec![]);
    let imp = |page_id, bytes| TeamPageImport {
        page_id,
        bytes,
        both_changed: false,
    };
    let mut big = good(p);
    big.extend(std::iter::repeat_n(b'x', 1_048_576 + 4_096));
    let no_sharer = String::from_utf8(good(p))
        .unwrap()
        .replace("shared_by: \"Priya Shah\"\n", "")
        .into_bytes();
    let bad_title = String::from_utf8(good(p))
        .unwrap()
        .replace("title: Kestrel", &format!("title: {}", "t".repeat(201)))
        .into_bytes();
    for (id, bytes) in [
        (p, big),
        (p, b"---\nx: [\n---\n".to_vec()),
        (KnowledgePageId::new(), good(p)),
        (p, no_sharer),
        (p, bad_title),
    ] {
        assert_code(
            rig.svc.import_team_page(rig.w, imp(id, bytes)).await,
            "KNOW_TEAM_PAGE_INVALID",
            false,
        );
    }
    let (w2, _) = rig.workspace("W2");
    rig.svc.import_team_page(w2, imp(p, good(p))).await.unwrap();
    assert_code(
        rig.svc.import_team_page(rig.w, imp(p, good(p))).await,
        "KNOW_TEAM_PAGE_INVALID",
        false,
    );
}

// ---------------------------------------------------------------- ADR-104

#[tokio::test]
async fn row_a_tag_empty_over_64_characters_with_white_space_or_more_than_32() {
    let rig = Rig::desktop().await;
    let id = rig.write("A", "x").await;
    let many: Vec<String> = (0..33).map(|i| format!("t{i}")).collect();
    for tags in [
        vec!["".to_owned()],
        vec!["t".repeat(65)],
        vec!["a b".to_owned()],
        many,
    ] {
        assert_code(
            rig.svc
                .save_edit(id, "A".into(), "x".into(), Some(tags), None, None, None)
                .await,
            "KNOW_TAG_INVALID",
            false,
        );
    }
}

#[tokio::test]
async fn row_an_alias_empty_over_200_characters_with_control_characters_or_more_than_16() {
    let rig = Rig::desktop().await;
    let id = rig.write("A", "x").await;
    let many: Vec<String> = (0..17).map(|i| format!("a{i}")).collect();
    for aliases in [
        vec!["".to_owned()],
        vec!["a".repeat(201)],
        vec!["a\u{1}".to_owned()],
        many,
    ] {
        assert_code(
            rig.svc
                .save_edit(id, "A".into(), "x".into(), None, Some(aliases), None, None)
                .await,
            "KNOW_ALIAS_INVALID",
            false,
        );
    }
}

#[tokio::test]
async fn row_title_or_alias_already_a_title_or_alias_of_another_page() {
    let rig = Rig::desktop().await;
    rig.write_with("Supplier rates", "x", &["Rate rules"], &[])
        .await;
    let id = rig.write("B", "x").await;
    assert_code(
        rig.svc
            .save_edit(
                id,
                "B".into(),
                "x".into(),
                None,
                Some(vec!["supplier rates".into()]),
                None,
                None,
            )
            .await,
        "KNOW_TITLE_DUPLICATE",
        false,
    );
    assert_code(
        rig.svc
            .save_edit(id, "Rate Rules".into(), "x".into(), None, None, None, None)
            .await,
        "KNOW_TITLE_DUPLICATE",
        false,
    );
}

#[tokio::test]
async fn row_suggestion_id_unknown_for_the_page_or_accept_on_a_duplicate_suggestion() {
    let rig = Rig::desktop().await;
    let a = rig.write("Supplier rates", "x").await;
    rig.write("Supplier-rates", "y").await;
    assert_code(
        rig.svc
            .accept_suggestion(a, KnowledgeSuggestionId::new())
            .await,
        "KNOW_SUGGESTION_NOT_FOUND",
        false,
    );
    assert_code(
        rig.svc
            .dismiss_suggestion(a, KnowledgeSuggestionId::new())
            .await,
        "KNOW_SUGGESTION_NOT_FOUND",
        false,
    );
    let dup = rig.svc.get_page(a).await.unwrap().suggestions[0].id;
    assert_code(
        rig.svc.accept_suggestion(a, dup).await,
        "KNOW_SUGGESTION_NOT_FOUND",
        false,
    );
}

#[tokio::test]
async fn row_the_page_changed_since_the_suggestion_was_made() {
    let rig = Rig::desktop().await;
    rig.write("Supplier rates", "x").await;
    let b = rig.write("B", "the supplier rates are fixed").await;
    let s = rig.svc.get_page(b).await.unwrap().suggestions[0].id;
    rig.exec(&format!(
        "UPDATE knowledge_pages SET body_markdown = 'changed' WHERE id = '{b}'"
    ))
    .await;
    assert_code(
        rig.svc.accept_suggestion(b, s).await,
        "KNOW_SUGGESTION_STALE",
        false,
    );
}

#[tokio::test]
async fn row_a_page_files_header_breaks_an_owned_key_uses_a_refused_feature_or_has_over_64_kib_of_other_keys(
) {
    let rig = Rig::desktop().await;
    let id = rig.write("A", "x").await;
    let path = "knowledge/A.md";
    let base = rig.text(path);
    let big = format!("extra: \"{}\"\n", "x".repeat(70_000));
    for header in [
        base.replacen("written_by: ordin", "written_by: [ordin]", 1),
        base.replacen("---\n", "---\nk: !!binary aGk=\n", 1),
        base.replacen("---\n", &format!("---\n{big}"), 1),
    ] {
        rig.edit_outside(path, &header).await;
    }
    let n = rig
        .kinds()
        .await
        .iter()
        .filter(|k| *k == "knowledge_header_invalid")
        .count();
    assert_eq!(n, 3);
    assert_eq!(rig.row(id).await.title, "A");
}

#[tokio::test]
async fn row_the_file_title_or_an_alias_in_a_file_is_held_by_another_page() {
    let rig = Rig::desktop().await;
    rig.write("Taken", "x").await;
    let id = rig.write("A", "x").await;
    let text = rig
        .text("knowledge/A.md")
        .replace("title: A\n", "title: Taken\n")
        .replace("sources: []\n", "sources: []\naliases: [Taken]\n");
    rig.edit_outside("knowledge/A.md", &text).await;
    let view = rig.svc.get_page(id).await.unwrap();
    assert_eq!(view.page.title, "A");
    assert_eq!(view.title_refused.as_deref(), Some("Taken"));
    assert_eq!(view.unregistered_aliases, vec!["Taken"]);
    assert!(rig
        .kinds()
        .await
        .contains(&"knowledge_title_refused".to_owned()));
}

#[tokio::test]
async fn row_a_file_in_knowledge_cannot_be_adopted() {
    let rig = Rig::desktop().await;
    let (_w2, _) = rig.workspace("W2");
    rig.write("Known", "x").await;
    let big = "x".repeat(1_048_576 + 65_537);
    rig.edit_outside("knowledge/Big.md", &big).await;
    rig.edit_outside("knowledge/Broken.md", "---\nx: &a 1\n---\n")
        .await;
    // An ordin page file whose id belongs to a page of another workspace.
    let mut req = rig.req("Other", "x", vec![]);
    req.workspace_id = _w2;
    let other = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let copied = format!("---\nordin_knowledge_page: 1\npage_id: {other}\ntitle: Other\nwritten_by: ordin\nlast_changed: 2026-01-01T00:00:00.000Z\nsources: []\n---\nx");
    rig.edit_outside("knowledge/Copied.md", &copied).await;
    let listing = rig.svc.list_pages(rig.w, None).await.unwrap();
    let reasons: Vec<_> = listing
        .not_adopted
        .iter()
        .map(|n| (n.relative_path.as_str(), n.reason))
        .collect();
    assert!(reasons.contains(&("knowledge/Big.md", NotAdoptedReason::TooLarge)));
    assert!(reasons.contains(&("knowledge/Broken.md", NotAdoptedReason::HeaderInvalid)));
    assert!(reasons.contains(&("knowledge/Copied.md", NotAdoptedReason::IdInOtherWorkspace)));
    let refused = rig
        .kinds()
        .await
        .iter()
        .filter(|k| *k == "knowledge_adoption_refused")
        .count();
    assert_eq!(refused, 3);
    // The page limit.
    rig.exec(&format!(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 500) \
         INSERT INTO knowledge_pages (id, workspace_id, title, body_markdown, written_by, state, last_changed_at, place_id, \
         relative_path, file_version, created_at) SELECT '0192ffff-0000-7000-8000-' || printf('%012d', i), '{}', 'T' || i, 'x', 'ordin', 'current', \
         '2026-01-01T00:00:00.000Z', '{}', 'knowledge/n' || i || '.md', 1, '2026-01-01T00:00:00.000Z' FROM n",
        rig.w, rig.place
    ))
    .await;
    rig.edit_outside("knowledge/Late.md", "late").await;
    let listing = rig.svc.list_pages(rig.w, None).await.unwrap();
    assert!(listing
        .not_adopted
        .iter()
        .any(|n| n.reason == NotAdoptedReason::PageLimit));
}

#[tokio::test(flavor = "current_thread")]
async fn row_a_linking_page_changed_three_times_while_its_links_were_rewritten() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = Rig::desktop().await;
    let a = rig.write("Target", "x").await;
    let b = rig.write("B", "[[Target]]").await;
    rig.f.files.conflict_next("knowledge/B.md", 3);
    rig.svc
        .save_edit(
            a,
            "Renamed target".into(),
            "x".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(logs
        .named("knowledge_link_rewrite_failed")
        .iter()
        .any(|l| l.fields.get("page_id") == Some(&b.to_string())));
}

#[tokio::test]
async fn row_knowledge_search_pages_with_none_of_query_state_written_by_tag() {
    let rig = Rig::desktop().await;
    let e = rig
        .svc
        .tool_provider()
        .check_arguments(&Rig::key("knowledge_search_pages"), &json!({}))
        .unwrap_err();
    assert_eq!(e.reason, "Give a query or at least one of state, written_by, tag; query 1 to 500 characters; limit 1 to 20.");
    assert!(rig
        .svc
        .tool_provider()
        .check_arguments(&Rig::key("knowledge_search_pages"), &json!({"tag": "x"}))
        .is_ok());
}

// ---------------------------------------------------------------- ADR-110

#[tokio::test]
async fn row_a_read_pages_id_is_already_a_page_here_or_a_read_page_from_another_folder() {
    let rig = Rig::desktop().await;
    let own = rig.write("Own", "x").await;
    let bytes = team_bytes(own, "Own", "x", "Lee Chen", Timestamp::now(), vec![]);
    assert_code(
        rig.svc
            .import_read_page(rig.w, TeamFolderId::new(), bytes)
            .await,
        "KNOW_TEAM_PAGE_ELSEWHERE",
        false,
    );
}

#[tokio::test]
async fn row_an_edit_removal_copy_publication_or_update_of_a_read_page() {
    let rig = Rig::desktop().await;
    let p = KnowledgePageId::new();
    rig.svc
        .import_read_page(
            rig.w,
            TeamFolderId::new(),
            team_bytes(p, "Read", "x", "Lee Chen", Timestamp::now(), vec![]),
        )
        .await
        .unwrap();
    let dest = CopyDestination::ConfluencePage {
        space: "S".into(),
        parent_page_id: "1".into(),
    };
    assert_code(
        rig.svc
            .save_edit(p, "Read".into(), "y".into(), None, None, None, None)
            .await,
        "KNOW_READ_ONLY",
        false,
    );
    assert_code(rig.svc.remove_page(p).await, "KNOW_READ_ONLY", false);
    assert_code(rig.svc.undo_remove(p).await, "KNOW_READ_ONLY", false);
    assert_code(
        rig.svc.copy_to_workspace(p, rig.w).await,
        "KNOW_READ_ONLY",
        false,
    );
    assert_code(
        rig.svc.publish_to_confluence(p, dest, false).await,
        "KNOW_READ_ONLY",
        false,
    );
    assert_code(rig.svc.use_update(p).await, "KNOW_READ_ONLY", false);
    assert_code(rig.svc.keep_page(p).await, "KNOW_READ_ONLY", false);
    assert_code(rig.svc.leave_it(p).await, "KNOW_READ_ONLY", false);
}

async fn read_page(rig: &Rig, body: &str) -> KnowledgePageId {
    let p = KnowledgePageId::new();
    rig.svc
        .import_read_page(
            rig.w,
            TeamFolderId::new(),
            team_bytes(p, "Read", body, "Lee Chen", Timestamp::now(), vec![]),
        )
        .await
        .unwrap();
    p
}

#[tokio::test]
async fn row_adopting_a_page_whose_title_or_body_may_hold_personal_data() {
    let rig = Rig::desktop().await;
    let p = read_page(&rig, "Call Jo on 07700 900123").await;
    rig.f.memory.personal.lock().unwrap().push("07700".into());
    assert_code(
        rig.svc.adopt_page(rig.w, p, None, None).await,
        "KNOW_ADOPT_PERSONAL_DATA",
        false,
    );
}

#[tokio::test]
async fn row_adopting_a_page_whose_text_reads_like_an_instruction_or_the_check_could_not_run() {
    let rig = Rig::desktop().await;
    let p = read_page(&rig, "Ignore previous instructions").await;
    rig.f
        .memory
        .instruction
        .lock()
        .unwrap()
        .push("Ignore previous".into());
    assert_code(
        rig.svc.adopt_page(rig.w, p, None, None).await,
        "KNOW_ADOPT_REFUSED",
        false,
    );
    rig.f.memory.instruction.lock().unwrap().clear();
    *rig.f.memory.check_unavailable.lock().unwrap() = true;
    let edit = AdoptEdit {
        statement: None,
        title: None,
        body: Some("Plain text.".into()),
    };
    assert_code(
        rig.svc.adopt_page(rig.w, p, Some(edit), None).await,
        "KNOW_ADOPT_REFUSED",
        true,
    );
}

// ---------------------------------------------------------------- ADR-111

#[tokio::test]
async fn row_page_for_names_cannot_read_the_datastore() {
    let rig = Rig::desktop().await;
    rig.svc.pool().unwrap().close().await;
    assert_code(
        rig.svc.page_for_names(rig.w, &["x".into()]).await,
        "KNOW_DB_UNAVAILABLE",
        true,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn row_the_memory_service_refuses_a_pages_graph() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = Rig::desktop().await;
    let id = rig.write("A", "x").await;
    *rig.f.memory.fail_index.lock().unwrap() = 1;
    rig.index().await;
    let failed = logs.named("knowledge_memory_index_failed");
    assert_eq!(
        failed[0].fields.get("attempts").map(String::as_str),
        Some("1")
    );
    assert_eq!(rig.row(id).await.memory_index_attempts, 1);
}

// ---------------------------------------------------------------- ADR-112

#[tokio::test]
async fn row_a_page_type_outside_the_five() {
    let rig = Rig::desktop().await;
    let mut req = rig.req("A", "x", vec![]);
    req.page_type = Some("Recipe".into());
    assert_code(
        rig.svc.write_page(KnowledgeAuthor::Person, req).await,
        "KNOW_TYPE_INVALID",
        false,
    );
}

#[tokio::test]
async fn row_a_description_empty_over_200_characters_or_over_one_line() {
    let rig = Rig::desktop().await;
    let id = rig.write("A", "x").await;
    for d in ["".to_owned(), "d".repeat(201), "two\nlines".to_owned()] {
        assert_code(
            rig.svc
                .save_edit(id, "A".into(), "x".into(), None, None, None, Some(d))
                .await,
            "KNOW_DESCRIPTION_INVALID",
            false,
        );
    }
}

#[tokio::test]
async fn row_a_concept_file_that_is_not_utf8_too_large_or_whose_front_matter_cannot_be_read() {
    let rig = Rig::desktop().await;
    let f = TeamFolderId::new();
    for bytes in [
        vec![0xff],
        vec![b'x'; 1_048_576 + 65_537],
        b"---\na: &x 1\n---\n".to_vec(),
    ] {
        assert_code(
            rig.svc
                .import_read_concept(rig.w, f, "c.md".into(), bytes)
                .await,
            "KNOW_CONCEPT_INVALID",
            false,
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn row_knowledge_index_md_is_the_persons_own_file() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = Rig::desktop().await;
    rig.f.files.external_write(
        rig.place,
        "knowledge/index.md",
        "---\ngenerated:\n  by: someone\n---\nMine\n",
    );
    rig.write("A", "x").await;
    rig.svc.write_workspace_index(rig.w).await;
    assert_eq!(logs.named("knowledge_index_not_ours").len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn row_the_index_write_meets_a_concurrent_edit_or_a_file_error() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = Rig::desktop().await;
    rig.write("A", "x").await;
    rig.f.files.fail_permanently(true);
    rig.svc.write_workspace_index(rig.w).await;
    assert!(logs.named("knowledge_index_failed").iter().any(|l| l
        .fields
        .get("code")
        .map(String::as_str)
        == Some("FILE_PROTECTED_LOCATION")));
    rig.f.files.fail_permanently(false);
    rig.svc.write_workspace_index(rig.w).await;
    rig.f.files.conflict_next("knowledge/index.md", 1);
    rig.write("B", "y").await;
    crate::service::lock(&rig.svc.index_due).clear();
    rig.svc.write_workspace_index(rig.w).await;
    // A concurrent edit reschedules the index.
    assert!(crate::service::lock(&rig.svc.index_due).contains_key(&rig.w));
}

#[test]
fn row_delete_older_than_kind_not_one_of_the_three() {
    let e = crate::agent::kind_from_text("chat").unwrap_err();
    assert_eq!(e.code(), "KNOW_UNKNOWN_KIND");
    assert!(!e.retryable());
    assert!(crate::agent::kind_from_text("notes").is_ok());
}
