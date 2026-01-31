//! Gherkin: "Knowledge Service — page links and editing in other apps" (amendment ADR-104).

use ordin_files::PlaceChangeKind;
use ordin_mcp_front::ToolCaller;
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePageState};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, KnowledgePageId};
use serde_json::json;

use super::rig::Rig;
use crate::model::{DuplicateReason, SuggestionKind};

const A_PATH: &str = "knowledge/Supplier rates.md";
const B_PATH: &str = "knowledge/Q3 review notes.md";

struct Bg {
    rig: Rig,
    a: KnowledgePageId,
    b: KnowledgePageId,
}

async fn background() -> Bg {
    let rig = Rig::desktop().await;
    let a = rig
        .write_with(
            "Supplier rates",
            "## Review cycle\nRates are reviewed each quarter.",
            &["Rate rules"],
            &[],
        )
        .await;
    let b = rig
        .write("Q3 review notes", "Notes of the third quarter.")
        .await;
    assert!(rig.f.files.text(rig.place, A_PATH).is_some());
    Bg { rig, a, b }
}

impl Bg {
    async fn save_b(&self, body: &str) {
        self.rig
            .svc
            .save_edit(
                self.b,
                "Q3 review notes".into(),
                body.into(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn happy_path_keys_written_by_another_app_survive() {
    let bg = background().await;
    let rig = &bg.rig;
    let text = rig
        .text(A_PATH)
        .replacen("---\n", "---\ncssclasses: [wide]\n", 1);
    let text = text.replace(
        "aliases: [\"Rate rules\"]\n",
        "tags: [suppliers]\naliases: [Rate rules]\n",
    );
    rig.edit_outside(A_PATH, &text).await;
    let view = rig.svc.get_page(bg.a).await.unwrap();
    assert_eq!(view.tags, vec!["suppliers"]);
    assert!(!view.page.body_markdown.contains("---"));
    assert!(!rig
        .kinds()
        .await
        .contains(&"knowledge_header_invalid".to_owned()));
    let mut req = rig.req("Supplier rates", "A new body.", vec![]);
    req.page_id = Some(bg.a);
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let file = rig.text(A_PATH);
    let header_end = file[4..].find("\n---\n").unwrap() + 4;
    let header = &file[..header_end];
    assert!(header.contains("\ncssclasses: [wide]"));
    assert!(header.find("cssclasses").unwrap() > header.find("aliases").unwrap());
}

#[tokio::test]
async fn error_a_refused_yaml_feature_is_still_refused() {
    let bg = background().await;
    let rig = &bg.rig;
    let text = rig.text(A_PATH).replacen("---\n", "---\nx: &x 1\n", 1);
    rig.edit_outside(A_PATH, &text).await;
    let invalid = rig
        .kinds()
        .await
        .iter()
        .filter(|k| *k == "knowledge_header_invalid")
        .count();
    assert_eq!(invalid, 1);
    let row = rig.row(bg.a).await;
    assert_eq!(row.title, "Supplier rates");
    assert_eq!(row.aliases, vec!["Rate rules"]);
}

#[tokio::test]
async fn happy_path_every_link_form_resolves() {
    let bg = background().await;
    bg.save_b("[[Supplier rates]] and [[rate rules|the rules]] and [[Supplier rates#Review cycle]] and [x](Supplier%20rates.md)")
        .await;
    let b = bg.rig.svc.get_page(bg.b).await.unwrap();
    assert_eq!(b.links_out.len(), 4);
    assert!(b
        .links_out
        .iter()
        .all(|l| l.target.as_ref().map(|t| t.page_id) == Some(bg.a)));
    assert_eq!(b.links_out[2].section.as_deref(), Some("Review cycle"));
    let a = bg.rig.svc.get_page(bg.a).await.unwrap();
    assert_eq!(a.links_in.len(), 1);
    assert_eq!(a.links_in[0].page_id, bg.b);
}

#[tokio::test]
async fn edge_links_in_code_are_not_links() {
    let bg = background().await;
    bg.save_b("```\n[[Supplier rates]]\n```\n").await;
    assert!(bg
        .rig
        .svc
        .get_page(bg.b)
        .await
        .unwrap()
        .links_out
        .is_empty());
}

#[tokio::test]
async fn edge_a_withheld_page_is_never_named_to_a_session() {
    let bg = background().await;
    let rig = &bg.rig;
    let (_, q) = rig.workspace("Elsewhere");
    let mut req = rig.req(
        "Q3 review notes",
        "See [[Supplier rates]].",
        vec![super::rig::src(&format!("place:{q}:secret.xlsx"), "1")],
    );
    req.page_id = Some(bg.b);
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let agent = AgentInstanceId::new();
    rig.f
        .rules
        .refused
        .lock()
        .unwrap()
        .insert((agent, format!("place:{q}:secret.xlsx")));
    let out = rig
        .call(
            rig.ctx(ToolCaller::Agent, agent),
            "knowledge_read_page",
            json!({"page_id": bg.a}),
        )
        .await
        .unwrap();
    assert_eq!(out["links_in"], json!([]));
    assert_eq!(out["withheld_links"], 1);
}

#[tokio::test]
async fn happy_path_a_rename_in_ordin_rewrites_linking_pages() {
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("See [[Supplier rates]].").await;
    rig.svc
        .save_edit(
            bg.a,
            "Supplier day rates".into(),
            rig.row(bg.a).await.body,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(rig.f.files.moves().contains(&(
        A_PATH.to_owned(),
        "knowledge/Supplier day rates.md".to_owned()
    )));
    let versions = rig.f.files.all_versions(rig.place, B_PATH);
    let last = String::from_utf8_lossy(&versions.last().unwrap().bytes).into_owned();
    let previous = String::from_utf8_lossy(&versions[versions.len() - 2].bytes).into_owned();
    assert!(last.contains("[[Supplier day rates]]"));
    assert_eq!(
        versions.last().unwrap().version.origin,
        ordin_files::FileVersionOrigin::Person
    );
    assert!(previous.contains("[[Supplier rates]]"));
}

#[tokio::test]
async fn edge_another_app_renames_a_file_and_updates_links_itself() {
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("See [[Supplier rates]].").await;
    let b_versions = rig.f.files.write_count(rig.place, B_PATH);
    rig.f
        .files
        .external_rename(rig.place, A_PATH, "knowledge/Day rates.md");
    rig.report(
        "knowledge/Day rates.md",
        PlaceChangeKind::Renamed {
            from: A_PATH.to_owned(),
        },
        None,
    )
    .await;
    let edited = rig
        .text(B_PATH)
        .replace("[[Supplier rates]]", "[[Day rates]]");
    rig.edit_outside(B_PATH, &edited).await;
    let b_after_app = rig.f.files.write_count(rig.place, B_PATH);
    // The rewrite falls due after RENAME_REWRITE_DELAY.
    let later = Timestamp(Timestamp::now().0 + chrono::Duration::seconds(11));
    rig.svc.run_due_rewrites(later).await.unwrap();
    let a = rig.row(bg.a).await;
    assert_eq!(a.title, "Day rates");
    assert_eq!(a.id, bg.a);
    assert_eq!(b_after_app, b_versions + 1);
    assert_eq!(rig.f.files.write_count(rig.place, B_PATH), b_after_app);
}

#[tokio::test(flavor = "current_thread")]
async fn edge_a_linking_page_edited_during_a_rewrite() {
    let (logs, _guard) = crate::tests::logs::capture();
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("See [[Supplier rates]].").await;
    rig.f.files.conflict_next(B_PATH, 3);
    rig.svc
        .save_edit(
            bg.a,
            "Supplier day rates".into(),
            rig.row(bg.a).await.body,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let failed = logs.named("knowledge_link_rewrite_failed");
    assert!(failed
        .iter()
        .any(|l| l.fields.get("page_id") == Some(&bg.b.to_string())));
    let b = rig.svc.get_page(bg.b).await.unwrap();
    assert_eq!(b.links_out.len(), 1);
    assert!(b.links_out[0].target.is_none());
}

#[tokio::test]
async fn edge_a_link_to_a_page_that_does_not_exist_yet() {
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("See [[Purchasing policy]].").await;
    let b = rig.svc.get_page(bg.b).await.unwrap();
    assert_eq!(b.links_out[0].name, "Purchasing policy");
    assert!(b.links_out[0].target.is_none());
    let writes = rig.f.files.write_count(rig.place, B_PATH);
    let p = rig.write("Purchasing policy", "Rules.").await;
    let b = rig.svc.get_page(bg.b).await.unwrap();
    assert_eq!(b.links_out[0].target.as_ref().unwrap().page_id, p);
    assert_eq!(rig.f.files.write_count(rig.place, B_PATH), writes);
}

#[tokio::test]
async fn error_an_alias_clashes_with_a_title() {
    let bg = background().await;
    let e = bg
        .rig
        .svc
        .write_page(
            KnowledgeAuthor::Person,
            bg.rig.req("rate rules", "x", vec![]),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_TITLE_DUPLICATE");
}

#[tokio::test]
async fn error_a_tag_with_a_space() {
    let bg = background().await;
    let e = bg
        .rig
        .svc
        .save_edit(
            bg.a,
            "Supplier rates".into(),
            "x".into(),
            Some(vec!["supplier rates".into()]),
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_TAG_INVALID");
}

#[tokio::test]
async fn happy_path_a_new_file_is_adopted() {
    let bg = background().await;
    let rig = &bg.rig;
    rig.edit_outside("knowledge/Onboarding notes.md", "Ask Sam for access.")
        .await;
    let listing = rig.svc.list_pages(rig.w, None).await.unwrap();
    let page = listing
        .this_workspace
        .iter()
        .find(|p| p.title == "Onboarding notes")
        .unwrap();
    assert_eq!(page.written_by, KnowledgeAuthor::Person);
    let row = rig.row(page.id).await;
    assert_eq!(row.body, "Ask Sam for access.");
    let file = rig.text("knowledge/Onboarding notes.md");
    assert!(file.starts_with("---\nordin_knowledge_page: 1\n"));
    assert!(file.contains(&format!("page_id: {}", page.id)));
    assert!(rig
        .kinds()
        .await
        .contains(&"knowledge_page_adopted".to_owned()));
}

#[tokio::test]
async fn edge_another_apps_settings_folder_is_ignored() {
    let bg = background().await;
    let rig = &bg.rig;
    let before = rig
        .svc
        .list_pages(rig.w, None)
        .await
        .unwrap()
        .this_workspace
        .len();
    rig.edit_outside("knowledge/.obsidian/app.json", "{}").await;
    let listing = rig.svc.list_pages(rig.w, None).await.unwrap();
    assert_eq!(listing.this_workspace.len(), before);
    assert!(listing.not_adopted.is_empty());
}

#[tokio::test]
async fn edge_a_page_file_deleted_in_another_app() {
    let bg = background().await;
    let rig = &bg.rig;
    let before = rig.text(A_PATH);
    rig.f.files.external_remove(rig.place, A_PATH);
    rig.report(A_PATH, PlaceChangeKind::Removed, None).await;
    assert_eq!(rig.row(bg.a).await.state, KnowledgePageState::Removed);
    rig.svc.undo_remove(bg.a).await.unwrap();
    assert_eq!(rig.text(A_PATH), before);
    assert_eq!(rig.row(bg.a).await.state, KnowledgePageState::Current);
}

/// ADR-112 changes the link written (an OKF Markdown link, SA-OKF-11) in place of ADR-104's
/// `[[Supplier rates|supplier rates]]`.
#[tokio::test]
async fn happy_path_a_link_suggestion_accepted() {
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("the supplier rates are fixed").await;
    let b = rig.svc.get_page(bg.b).await.unwrap();
    let links: Vec<_> = b
        .suggestions
        .iter()
        .filter(|s| s.kind == SuggestionKind::Link)
        .collect();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].other.page_id, bg.a);
    assert_eq!(links[0].matched_text.as_deref(), Some("supplier rates"));
    rig.svc.accept_suggestion(bg.b, links[0].id).await.unwrap();
    let file = rig.text(B_PATH);
    assert!(
        file.contains("the [supplier rates](/Supplier%20rates.md) are fixed"),
        "{file}"
    );
    let last = rig.f.files.all_versions(rig.place, B_PATH).pop().unwrap();
    assert_eq!(last.version.origin, ordin_files::FileVersionOrigin::Person);
}

#[tokio::test]
async fn error_a_stale_suggestion() {
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("the supplier rates are fixed").await;
    let s = rig.svc.get_page(bg.b).await.unwrap().suggestions[0].id;
    rig.exec(&format!("UPDATE knowledge_pages SET body_markdown = 'the supplier rates are fixed, mostly' WHERE id = '{}'", bg.b))
        .await;
    let file = rig.text(B_PATH);
    let e = rig.svc.accept_suggestion(bg.b, s).await.unwrap_err();
    assert_eq!(e.code(), "KNOW_SUGGESTION_STALE");
    assert_eq!(rig.text(B_PATH), file);
}

#[tokio::test]
async fn edge_a_dismissed_suggestion_stays_dismissed() {
    let bg = background().await;
    let rig = &bg.rig;
    bg.save_b("the supplier rates are fixed").await;
    let s = rig.svc.get_page(bg.b).await.unwrap().suggestions[0].id;
    rig.svc.dismiss_suggestion(bg.b, s).await.unwrap();
    bg.save_b("the supplier rates are fixed. A new line elsewhere.")
        .await;
    let b = rig.svc.get_page(bg.b).await.unwrap();
    assert!(!b
        .suggestions
        .iter()
        .any(|s| s.kind == SuggestionKind::Link && s.other.page_id == bg.a));
}

#[tokio::test]
async fn edge_likely_duplicates_are_flagged_never_merged() {
    let bg = background().await;
    let rig = &bg.rig;
    let a_before = rig.text(A_PATH);
    let c = rig.write("Supplier-rates", "Another page.").await;
    let a = rig.svc.get_page(bg.a).await.unwrap();
    let cv = rig.svc.get_page(c).await.unwrap();
    let dup = |v: &crate::model::PageView, other| {
        v.suggestions.iter().any(|s| {
            s.kind == SuggestionKind::Duplicate
                && s.other.page_id == other
                && s.reason == Some(DuplicateReason::Names)
        })
    };
    assert!(dup(&a, c));
    assert!(dup(&cv, bg.a));
    assert_eq!(rig.text(A_PATH), a_before);
}

#[tokio::test]
async fn happy_path_search_filters() {
    let bg = background().await;
    let rig = &bg.rig;
    rig.f.files.external_write(rig.place, "rates.xlsx", "v1");
    let mut req = rig.req(
        "Supplier rates",
        "Rates are reviewed.",
        vec![rig.place_src("rates.xlsx", "1")],
    );
    req.page_id = Some(bg.a);
    req.tags = Some(vec!["suppliers".into()]);
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let mut req = rig.req("Q3 review notes", "Rates of the quarter.", vec![]);
    req.page_id = Some(bg.b);
    req.tags = Some(vec!["suppliers".into()]);
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    rig.report("rates.xlsx", PlaceChangeKind::Modified, Some(2))
        .await;
    let out = rig
        .call(
            rig.ctx(ToolCaller::Ordin, AgentInstanceId::new()),
            "knowledge_search_pages",
            json!({"query":"rates","state":"may_be_out_of_date","tag":"suppliers"}),
        )
        .await
        .unwrap();
    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["page_id"], json!(bg.a));
    assert_eq!(results[0]["tags"], json!(["suppliers"]));
    let listing = rig
        .svc
        .list_pages(rig.w, Some("tag:suppliers state:out-of-date rates".into()))
        .await
        .unwrap();
    assert_eq!(listing.this_workspace.len(), 1);
    assert_eq!(listing.this_workspace[0].id, bg.a);
}

#[tokio::test]
async fn error_a_search_with_nothing_to_search_by() {
    let bg = background().await;
    let provider = bg.rig.svc.tool_provider();
    let e = provider
        .check_arguments(&Rig::key("knowledge_search_pages"), &json!({"limit": 5}))
        .unwrap_err();
    assert_eq!(
        e.reason,
        "Give a query or at least one of state, written_by, tag; query 1 to 500 characters; limit 1 to 20."
    );
}

#[tokio::test]
async fn happy_path_upgrade_builds_links_once() {
    let rig = Rig::desktop().await;
    let a = rig.write("Alpha", "First page.").await;
    let b = rig.write("Beta", "Links to [[Alpha]].").await;
    let c = rig.write("Gamma", "Third page.").await;
    // As written before the amendment: no links built, names seeded with SQLite's `lower`.
    rig.exec("UPDATE knowledge_pages SET links_built_version = NULL")
        .await;
    rig.exec("DELETE FROM knowledge_links").await;
    rig.exec("DELETE FROM knowledge_page_names").await;
    rig.exec("INSERT INTO knowledge_page_names (workspace_id, name_key, page_id, kind) SELECT workspace_id, lower(trim(title)), id, 'title' FROM knowledge_pages").await;
    let moves = rig.f.files.moves().len();
    rig.svc.startup_scan().await.unwrap();
    for id in [a, b, c] {
        let row = rig.row(id).await;
        assert_eq!(row.links_built_version, Some(row.file_version));
    }
    let view = rig.svc.get_page(b).await.unwrap();
    assert_eq!(view.links_out[0].target.as_ref().unwrap().page_id, a);
    assert_eq!(rig.f.files.moves().len(), moves);
}
