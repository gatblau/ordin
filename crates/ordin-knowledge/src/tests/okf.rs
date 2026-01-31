//! Gherkin: "pages in the memory graph" (ADR-111) and "the wiki as an Open Knowledge Format
//! bundle" (ADR-112).

use ordin_files::PlaceChangeKind;
use ordin_mcp_front::ToolCaller;
use ordin_memory_service::PageStanding;
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePageState};
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::yaml::{read_restricted_node, YamlNode};
use ordin_types::{AgentInstanceId, KnowledgePageId, TeamFolderId};
use serde_json::json;

use super::rig::Rig;
use super::team::team_bytes;
use crate::service::KnowledgeService;

// ---------------------------------------------------------------- ADR-111

#[tokio::test]
async fn happy_path_a_pages_title_and_other_names_reach_memory() {
    let rig = Rig::desktop().await;
    let p = rig
        .write_with("Supplier Borealis", "Our supplier.", &["Borealis"], &[])
        .await;
    rig.index().await;
    let call = rig.f.memory.calls_for(p).pop().unwrap();
    assert_eq!(call.graph.title, "Supplier Borealis");
    assert_eq!(call.graph.other_names, vec!["Borealis"]);
}

#[tokio::test]
async fn happy_path_explained_in() {
    let rig = Rig::desktop().await;
    let p = rig
        .write_with("Supplier Borealis", "Our supplier.", &["Borealis"], &[])
        .await;
    let m = rig
        .svc
        .page_for_names(rig.w, &["Borealis".into()])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(m.page_id, p);
    assert_eq!(m.title, "Supplier Borealis");
}

#[tokio::test]
async fn edge_own_page_before_a_read_page() {
    let rig = Rig::desktop().await;
    let own = rig.write("Release checklist", "Ours.").await;
    let q = KnowledgePageId::new();
    let bytes = team_bytes(
        q,
        "Release checklist",
        "Theirs.",
        "Lee Chen",
        Timestamp::now(),
        vec![],
    );
    rig.svc
        .import_read_page(rig.w, TeamFolderId::new(), bytes)
        .await
        .unwrap();
    let m = rig
        .svc
        .page_for_names(rig.w, &["release checklist".into()])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(m.page_id, own);
}

#[tokio::test]
async fn edge_no_page_explains_the_thing() {
    let rig = Rig::desktop().await;
    rig.write("Release checklist", "Ours.").await;
    assert!(rig
        .svc
        .page_for_names(rig.w, &["Kestrel".into()])
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn happy_path_a_link_between_pages_becomes_a_link_between_things() {
    let rig = Rig::desktop().await;
    let p = rig.write("Supplier Borealis", "Our supplier.").await;
    let a = rig.write("A", "See [[Supplier Borealis]].").await;
    rig.index().await;
    let call = rig.f.memory.calls_for(a).pop().unwrap();
    assert_eq!(
        call.graph.links_to,
        vec![(p, "Supplier Borealis".to_owned())]
    );
}

#[tokio::test]
async fn happy_path_removing_the_link_closes_it_at_the_next_version() {
    let rig = Rig::desktop().await;
    rig.write("Supplier Borealis", "Our supplier.").await;
    let a = rig.write("A", "See [[Supplier Borealis]].").await;
    rig.index().await;
    rig.svc
        .save_edit(a, "A".into(), "No link now.".into(), None, None, None, None)
        .await
        .unwrap();
    rig.index().await;
    let call = rig.f.memory.calls_for(a).pop().unwrap();
    assert_eq!(call.version, rig.row(a).await.file_version.to_string());
    assert!(call.graph.links_to.is_empty());
}

#[tokio::test]
async fn edge_a_removed_target_page_reindexes_the_pages_that_link_to_it() {
    let rig = Rig::desktop().await;
    let p = rig.write("Supplier Borealis", "Our supplier.").await;
    let a = rig.write("A", "See [[Supplier Borealis]].").await;
    rig.index().await;
    rig.svc.remove_page(p).await.unwrap();
    assert_eq!(rig.row(a).await.memory_indexed_version, None);
    rig.index().await;
    assert!(rig
        .f
        .memory
        .calls_for(a)
        .pop()
        .unwrap()
        .graph
        .links_to
        .is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn edge_at_most_15_links() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = Rig::desktop().await;
    let mut body = String::new();
    let mut targets = Vec::new();
    for i in 0..20 {
        targets.push(rig.write(&format!("Target {i:02}"), "x").await);
        body.push_str(&format!("[[Target {i:02}]] "));
    }
    let a = rig.write("A", &body).await;
    // Twenty pages a pass: the 21st page is indexed by the second pass.
    rig.index().await;
    rig.index().await;
    let call = rig.f.memory.calls_for(a).pop().unwrap();
    let ids: Vec<_> = call.graph.links_to.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, targets[..15].to_vec());
    let capped = logs.named("knowledge_page_links_capped");
    assert!(capped
        .iter()
        .any(|l| l.fields.get("left_out").map(String::as_str) == Some("5")));
}

#[tokio::test]
async fn happy_path_a_read_pages_links_stay_in_its_folder() {
    let rig = Rig::desktop().await;
    let b = TeamFolderId::new();
    let r = KnowledgePageId::new();
    rig.svc
        .import_read_page(
            rig.w,
            b,
            team_bytes(
                r,
                "Rollback steps",
                "Steps.",
                "Lee Chen",
                Timestamp::now(),
                vec![],
            ),
        )
        .await
        .unwrap();
    let q = KnowledgePageId::new();
    rig.svc
        .import_read_page(
            rig.w,
            b,
            team_bytes(
                q,
                "Q",
                "See [[Rollback steps]].",
                "Lee Chen",
                Timestamp::now(),
                vec![],
            ),
        )
        .await
        .unwrap();
    rig.index().await;
    let call = rig.f.memory.calls_for(q).pop().unwrap();
    assert_eq!(call.folder_id, Some(b));
    assert_eq!(call.graph.links_to, vec![(r, "Rollback steps".to_owned())]);
}

// ---------------------------------------------------------------- ADR-112

/// An OKF 0.2 checker written from the specification: front matter parses and `type` is a
/// non-empty string.
fn conformant(text: &str) -> bool {
    let Ok((Some(header), _)) = crate::okf::split_front_matter(text) else {
        return false;
    };
    match read_restricted_node(header, 1 << 20) {
        Ok(YamlNode::Map(m)) => m.iter().any(|(k, v)| {
            k == "type" && matches!(v, YamlNode::Scalar { text, .. } if !text.is_empty())
        }),
        _ => false,
    }
}

async fn okf_rig() -> Rig {
    let rig = Rig::desktop().await;
    *rig.f.identity.login.lock().unwrap() = Some("asha-patel".into());
    rig
}

#[tokio::test]
async fn happy_path_every_page_ordin_writes_is_a_conformant_concept() {
    let rig = okf_rig().await;
    let mut req = rig.req("How supplier rates are set", "Rates.", vec![]);
    req.description = Some("How supplier day rates are set and reviewed each quarter.".into());
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let mut req = rig.req("Purchasing policy", "Policy.", vec![]);
    req.page_type = Some("Reference".into());
    rig.svc
        .write_page(KnowledgeAuthor::Person, req)
        .await
        .unwrap();
    let first = rig.text("knowledge/How supplier rates are set.md");
    let second = rig.text("knowledge/Purchasing policy.md");
    assert!(conformant(&first) && conformant(&second));
    assert!(first.contains("type: Explanation\n"));
    assert!(first
        .contains("description: \"How supplier day rates are set and reviewed each quarter.\"\n"));
    assert!(first.contains("  by: \"ordin/0.61.0\"\n"));
    assert!(second.contains("  - by: \"human:asha-patel\"\n"));
}

#[tokio::test]
async fn edge_a_page_written_before_this_change_opens_unchanged() {
    let rig = okf_rig().await;
    let id = rig.write("Old page", "Old body.").await;
    // The file and row as an earlier build wrote them.
    let path = "knowledge/Old page.md";
    let old = format!(
        "---\nordin_knowledge_page: 1\npage_id: {id}\ntitle: Old page\nwritten_by: ordin\nlast_changed: 2026-01-01T00:00:00.000Z\nsources:\n  - origin: place\n    location: \"place:{}:a.txt\"\n    version_read: \"1\"\ncssclasses: [wide]\n---\nOld body.",
        rig.place
    );
    let v = rig.f.files.external_write(rig.place, path, &old);
    rig.report(path, PlaceChangeKind::Modified, Some(v.version))
        .await;
    rig.exec("UPDATE knowledge_pages SET page_type = NULL, description = NULL, verified = '[]', okf_tier = 'unverified', generated = NULL").await;
    rig.f.files.external_write(rig.place, "a.txt", "x");
    let writes = rig.f.files.write_count(rig.place, path);
    let view = rig.svc.get_page(id).await.unwrap();
    assert_eq!(view.trust, PageTier::Unverified);
    assert!(view.page_type.is_none());
    assert_eq!(rig.f.files.write_count(rig.place, path), writes);
    rig.svc
        .save_edit(
            id,
            "Old page".into(),
            "A new body.".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let file = rig.text(path);
    assert!(file.contains("type: Explanation\n"));
    assert!(file.contains("description: "));
    assert!(file.contains("generated:\n"));
    assert!(
        file.contains("    resource: ")
            && file.contains("    id: ")
            && file.contains("    last_modified: ")
    );
    assert!(file.contains("cssclasses: [wide]\n"));
    for key in [
        "ordin_knowledge_page",
        "page_id",
        "title",
        "written_by",
        "last_changed",
        "sources",
    ] {
        assert!(file.contains(&format!("{key}:")), "{key}");
    }
}

#[tokio::test]
async fn happy_path_another_producers_bundle_is_read_with_no_error() {
    let rig = okf_rig().await;
    let b = TeamFolderId::new();
    let text = "---\ntype: Supplier Profile\nowner_team: buying\n---\nBorealis supplies packaging. See [Packaging](/missing.md).\n";
    let got = rig
        .svc
        .import_read_concept(
            rig.w,
            b,
            "suppliers/borealis.md".into(),
            text.as_bytes().to_vec(),
        )
        .await
        .unwrap();
    assert_eq!(got, crate::model::TeamPageImported::Written { version: 1 });
    let id = KnowledgeService::concept_page_id(b, "suppliers/borealis.md");
    let view = rig.svc.get_page(id).await.unwrap();
    assert_eq!(view.page_type.as_deref(), Some("Supplier Profile"));
    assert_eq!(view.links_out.len(), 1);
    assert_eq!(view.links_out[0].name, "missing");
    assert!(view.links_out[0].target.is_none());
}

#[tokio::test]
async fn edge_another_producers_fields_survive_a_rewrite_byte_for_byte() {
    let rig = okf_rig().await;
    let id = rig.write("Borealis", "Body.").await;
    let path = "knowledge/Borealis.md";
    let text = rig.text(path);
    let text = text.replace("sources: []\n", &format!("sources:\n  - origin: place\n    location: \"place:{}:c.pdf\"\n    version_read: \"1\"\n    author: buying-team\n    title: Contract 2026\n", rig.place));
    let text = text.replacen("---\n", "---\nx-review: q4\n", 1);
    rig.edit_outside(path, &text).await;
    rig.svc
        .save_edit(
            id,
            "Borealis".into(),
            "New body.".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let file = rig.text(path);
    assert!(
        file.contains("    author: buying-team\n    title: Contract 2026\n"),
        "{file}"
    );
    assert!(file.contains("x-review: q4\n"));
}

/// The desktop's what-you-see editor sends only the title and the body it wrote (amended January
/// 2026, user decision): the header (type, description, tags, sources, who wrote it, who checked it
/// and keys ordin does not know) comes back as it was, and the body is written as given, its
/// `[[Page]]` links and Markdown links unchanged.
#[tokio::test]
async fn edge_a_save_from_the_editor_keeps_the_header_and_the_body() {
    let rig = okf_rig().await;
    let mut req = rig.req("Payment terms", "Thirty days.", vec![]);
    req.page_type = Some("How-to".into());
    req.description = Some("How to agree payment terms with a supplier.".into());
    let id = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let path = "knowledge/Payment terms.md";
    let text = rig.text(path);
    let text = text.replace(
        "sources: []\n",
        &format!(
            "sources:\n  - origin: place\n    location: \"place:{}:terms.pdf\"\n    version_read: \"1\"\n    author: buying-team\ntags: [finance, suppliers]\n",
            rig.place
        ),
    );
    let text = text.replacen("---\n", "---\nowner_team: buying\n", 1);
    rig.edit_outside(path, &text).await;
    // What the editor gives out for a page with a heading, a list and both kinds of link.
    let body = "## Late fees\n\nSee [[Supplier Borealis]] and [[Rates|the rates]].\n\n- Read [the policy](policy.md)\n- Or [the site](https://example.com/terms)\n";
    rig.svc
        .save_edit(
            id,
            "Payment terms".into(),
            body.into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let file = rig.text(path);
    for line in [
        "type: How-to\n",
        "description: \"How to agree payment terms with a supplier.\"\n",
        "tags: [\"finance\", \"suppliers\"]\n",
        "    author: buying-team\n",
        "owner_team: buying\n",
        "generated:\n",
        "  - by: \"human:asha-patel\"\n",
    ] {
        assert!(file.contains(line), "{line} in {file}");
    }
    assert!(file.ends_with(&format!("---\n{body}")), "{file}");
}

#[tokio::test]
async fn edge_indexmd_and_logmd_are_never_pages() {
    let rig = okf_rig().await;
    rig.write("Some page", "x").await;
    rig.edit_outside("knowledge/index.md", "# My index").await;
    rig.edit_outside("knowledge/log.md", "# My log").await;
    let listing = rig.svc.list_pages(rig.w, None).await.unwrap();
    assert_eq!(listing.this_workspace.len(), 1);
    assert!(listing.not_adopted.is_empty());
}

#[tokio::test]
async fn happy_path_ordin_keeps_the_workspace_index() {
    let rig = okf_rig().await;
    for (title, d) in [
        ("Purchasing policy", "Rules every purchase follows."),
        ("Supplier rates", "How rates are set."),
    ] {
        let mut req = rig.req(title, "x", vec![]);
        req.description = Some(d.into());
        rig.svc
            .write_page(KnowledgeAuthor::Person, req)
            .await
            .unwrap();
    }
    let mut req = rig.req("Borealis", "x", vec![]);
    req.description = Some("Our packaging supplier.".into());
    rig.svc
        .write_page(KnowledgeAuthor::Person, req)
        .await
        .unwrap();
    let due = *crate::service::lock(&rig.svc.index_due)
        .get(&rig.w)
        .unwrap();
    assert!(due <= std::time::Instant::now() + std::time::Duration::from_secs(6));
    rig.svc.write_workspace_index(rig.w).await;
    let index = rig.text("knowledge/index.md");
    assert!(index.contains("okf_version: \"0.2\""));
    let bullets: Vec<&str> = index.lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(bullets.len(), 3);
    assert_eq!(
        bullets[0],
        "- [Borealis](/Borealis.md) — Our packaging supplier."
    );
}

#[tokio::test(flavor = "current_thread")]
async fn edge_the_persons_own_index_is_left_alone() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = okf_rig().await;
    rig.f.files.external_write(
        rig.place,
        "knowledge/index.md",
        "# The person's own index\n",
    );
    rig.write("A page", "x").await;
    rig.svc.write_workspace_index(rig.w).await;
    assert_eq!(rig.text("knowledge/index.md"), "# The person's own index\n");
    assert_eq!(logs.named("knowledge_index_not_ours").len(), 1);
}

#[tokio::test]
async fn happy_path_links_ordin_writes_are_okf_links_the_persons_wiki_links_stay() {
    let rig = okf_rig().await;
    rig.write("Supplier rates", "Rates.").await;
    rig.write("Purchasing policy", "Policy.").await;
    let b = rig
        .write(
            "B",
            "the supplier rates are fixed\n\nsee [[Purchasing policy]]",
        )
        .await;
    let view = rig.svc.get_page(b).await.unwrap();
    let s = view
        .suggestions
        .iter()
        .find(|s| s.matched_text.as_deref() == Some("supplier rates"))
        .unwrap();
    rig.svc.accept_suggestion(b, s.id).await.unwrap();
    let file = rig.text("knowledge/B.md");
    assert!(file.contains("the [supplier rates](/Supplier%20rates.md) are fixed"));
    assert!(file.contains("see [[Purchasing policy]]"));
}

#[tokio::test]
async fn edge_a_rename_keeps_each_links_form() {
    let rig = okf_rig().await;
    let a = rig.write("Supplier rates", "Rates.").await;
    rig.write("B", "[[Supplier rates]] and [rates](/Supplier%20rates.md)")
        .await;
    rig.svc
        .save_edit(
            a,
            "Supplier day rates".into(),
            "Rates.".into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let file = rig.text("knowledge/B.md");
    assert!(file.contains("[[Supplier day rates]]"), "{file}");
    assert!(
        file.contains("[rates](/Supplier%20day%20rates.md)"),
        "{file}"
    );
}

#[tokio::test]
async fn happy_path_trust_and_status_reach_memory() {
    let rig = okf_rig().await;
    let a = rig.write("A", "x").await;
    let text = rig
        .text("knowledge/A.md")
        .replacen("---\n", "---\nstatus: draft\n", 1);
    let text = text.replace(
        "sources: []\n",
        "verified:\n  - by: \"human:asha-patel\"\nsources: []\n",
    );
    rig.edit_outside("knowledge/A.md", &text).await;
    rig.index().await;
    let call = rig.f.memory.calls_for(a).pop().unwrap();
    assert_eq!(
        call.graph.standing,
        PageStanding {
            tier: PageTier::HumanReviewed,
            status: PageStatus::Draft,
            past_stale_after: false
        }
    );
}

#[tokio::test]
async fn edge_stale_after_passes() {
    let rig = okf_rig().await;
    let a = rig.write("A", "x").await;
    let text = rig
        .text("knowledge/A.md")
        .replacen("---\n", "---\nstale_after: 2026-01-10\n", 1);
    rig.edit_outside("knowledge/A.md", &text).await;
    let view = rig.svc.get_page(a).await.unwrap();
    assert!(view.past_stale_after);
    assert_eq!(view.page.state, KnowledgePageState::Current);
    rig.index().await;
    assert!(
        rig.f
            .memory
            .calls_for(a)
            .pop()
            .unwrap()
            .graph
            .standing
            .past_stale_after
    );
}

#[tokio::test]
async fn edge_deprecated_pages_only_when_nothing_else_matches() {
    let rig = okf_rig().await;
    let old = rig
        .write("Old rate card", "The rate card of last year.")
        .await;
    let new = rig.write("Rate card", "The rate card.").await;
    let text =
        rig.text("knowledge/Old rate card.md")
            .replacen("---\n", "---\nstatus: deprecated\n", 1);
    rig.edit_outside("knowledge/Old rate card.md", &text).await;
    let ctx = || rig.ctx(ToolCaller::Ordin, AgentInstanceId::new());
    let out = rig
        .call(
            ctx(),
            "knowledge_search_pages",
            json!({"query": "rate card"}),
        )
        .await
        .unwrap();
    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["page_id"], json!(new));
    rig.svc.remove_page(new).await.unwrap();
    let out = rig
        .call(
            ctx(),
            "knowledge_search_pages",
            json!({"query": "rate card"}),
        )
        .await
        .unwrap();
    assert_eq!(out["results"][0]["page_id"], json!(old));
    assert_eq!(out["results"][0]["status"], "deprecated");
}

#[tokio::test]
async fn error_a_type_outside_the_list() {
    let rig = okf_rig().await;
    let a = rig.write("A", "x").await;
    let before = rig.text("knowledge/A.md");
    let e = rig
        .svc
        .save_edit(
            a,
            "A".into(),
            "y".into(),
            None,
            None,
            Some("Recipe".into()),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_TYPE_INVALID");
    assert_eq!(rig.text("knowledge/A.md"), before);
}

#[tokio::test]
async fn error_a_concept_that_cannot_be_read() {
    let rig = okf_rig().await;
    let e = rig
        .svc
        .import_read_concept(
            rig.w,
            TeamFolderId::new(),
            "a.md".into(),
            vec![0xff, 0xfe, 0x00],
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_CONCEPT_INVALID");
    assert!(rig
        .svc
        .list_read_pages(rig.w, None)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn edge_a_page_that_was_adopted_under_a_reserved_name() {
    let rig = okf_rig().await;
    let id = rig.write("Index", "x").await;
    // As adopted before this amendment: the row's file is `knowledge/Index.md`.
    rig.f
        .files
        .external_rename(rig.place, "knowledge/Index page.md", "knowledge/Index.md");
    rig.exec(&format!(
        "UPDATE knowledge_pages SET relative_path = 'knowledge/Index.md' WHERE id = '{id}'"
    ))
    .await;
    rig.svc.startup_scan().await.unwrap();
    assert!(rig.f.files.moves().contains(&(
        "knowledge/Index.md".to_owned(),
        "knowledge/Index page.md".to_owned()
    )));
    assert_eq!(rig.row(id).await.relative_path, "knowledge/Index page.md");
    assert!(rig
        .kinds()
        .await
        .contains(&"knowledge_page_moved_from_reserved".to_owned()));
}
