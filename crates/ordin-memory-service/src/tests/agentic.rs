//! The scenarios of the ADR-111 amendment (agentic memory), each a test of the same name.

use ordin_mcp_front::{ProgramProfile, SessionGrant, ToolCaller};
use ordin_types::domain::identity::HarnessKind;
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, KnowledgePageId, MemoryFactId, MemoryThingId, RunId};
use serde_json::json;

use super::support::*;
use crate::db::q;
use crate::model::*;

fn borealis_input(statement: &str) -> RememberInput {
    RememberInput {
        entities: vec![thing("Borealis"), thing("packaging")],
        links: vec![link("Borealis", "supplies", "packaging")],
        ..input(statement, None)
    }
}

/// The thing of a name in a place, by its listing. The memory component stores a canonical name
/// normalised (lower case), so names compare without case.
async fn thing_named(rig: &Rig, w: ordin_types::WorkspaceId, name: &str) -> ThingView {
    rig.svc
        .list_things(MemoryPlace::Workspace(w), Some(name.to_owned()), None)
        .await
        .expect("things")
        .items
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .expect("the thing")
}

async fn borealis_fact(rig: &Rig) -> MemoryFactId {
    let receipt = rig
        .svc
        .remember_text(
            rig.ordin(rig.w1),
            borealis_input("Borealis supplies packaging"),
        )
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    receipt.id
}

#[tokio::test]
async fn happy_path_a_fact_with_things_and_a_link() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    let args = json!({
        "statement": "Borealis supplies packaging",
        "entities": [{"name": "Borealis"}, {"name": "packaging"}],
        "links": [{"from": "Borealis", "kind": "supplies", "to": "packaging"}],
    });
    let run = RunId::new();
    let caller = ToolCaller::CrewMember {
        crew_run_id: ordin_types::CrewRunId::new(),
    };
    let first = rig
        .call_in_run("remember", caller, run, rig.w1, agent, None, args.clone())
        .await
        .expect("remember");
    let id = MemoryFactId::parse(
        first.structured.as_ref().expect("structured")["id"]
            .as_str()
            .expect("id"),
    )
    .expect("id");
    assert_eq!(rig.keep_held_writes(rig.w1, 1).await.len(), 1);
    assert_eq!(rig.state(id).await, FactState::NotKept);
    let found = thing_named(&rig, rig.w1, "Borealis").await;
    let detail = rig
        .svc
        .get_thing(MemoryPlace::Workspace(rig.w1), found.id)
        .await
        .expect("thing");
    assert_eq!(detail.facts.len(), 1);
    assert_eq!(detail.links.len(), 1);
    assert_eq!(detail.links[0].kind, "supplies");
    assert_eq!(detail.links[0].from.name, "borealis");
    assert_eq!(detail.links[0].to.name, "packaging");
    assert_eq!(detail.links[0].until, None);
    assert!(matches!(detail.links[0].by, LinkBy::Agent { .. }));

    let counts = (
        rig.count("SELECT COUNT(*) AS n FROM mem_fact").await,
        rig.count("SELECT COUNT(*) AS n FROM mem_entity").await,
        rig.count("SELECT COUNT(*) AS n FROM mem_relationship")
            .await,
    );
    assert_eq!(counts, (1, 2, 1));
    let again = rig
        .call_in_run("remember", caller, run, rig.w1, agent, None, args)
        .await
        .expect("again");
    assert_eq!(
        again.structured.expect("structured")["id"],
        first.structured.expect("structured")["id"]
    );
    rig.settle().await;
    assert_eq!(
        (
            rig.count("SELECT COUNT(*) AS n FROM mem_fact").await,
            rig.count("SELECT COUNT(*) AS n FROM mem_entity").await,
            rig.count("SELECT COUNT(*) AS n FROM mem_relationship")
                .await,
        ),
        counts
    );
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_link_that_is_no_longer_true_is_closed_and_kept_in_history_criterion_2() {
    let rig = Rig::desktop().await;
    borealis_fact(&rig).await;
    let mut ended = borealis_input("Borealis no longer supplies packaging");
    ended.links[0].ended = true;
    let receipt = rig
        .svc
        .remember_text(rig.ordin(rig.w1), ended)
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    let history = rig.svc.fact_history(receipt.id).await.expect("history");
    assert!(history
        .links
        .iter()
        .any(|l| l.kind == "supplies" && l.until.is_some()));
    let result = rig.recall_text(rig.w1, "who supplies packaging").await;
    assert!(!result
        .links
        .iter()
        .any(|l| l.kind == "supplies" && l.until.is_none()));
    rig.stop().await;
}

/// `fN` facts in bulk, through a team get.
async fn filler(rig: &Rig, count: usize, word: &str) {
    for chunk in 0..count.div_ceil(200) {
        let mut facts = Vec::new();
        let mut entities = Vec::new();
        for n in 0..200.min(count - chunk * 200) {
            let (fact, entity) = team_fact(&format!("{word}{chunk}x{n} files a monthly report."));
            facts.push(fact);
            entities.push(entity);
        }
        rig.svc
            .import_team(
                rig.w1,
                TeamImportBatch {
                    terms: Vec::new(),
                    facts,
                    entities,
                    sources: Vec::new(),
                    relationships: Vec::new(),
                    read_folder: None,
                },
            )
            .await
            .expect("filler");
    }
}

#[tokio::test]
async fn happy_path_recall_finds_a_thing_the_question_names_criterion_3() {
    let rig = Rig::desktop().await;
    filler(&rig, 999, "Vendor").await;
    let receipt = rig
        .svc
        .remember_text(
            rig.ordin(rig.w1),
            RememberInput {
                entities: vec![thing("Borealis"), thing("packaging")],
                links: vec![link("Borealis", "provides", "packaging")],
                ..input("Packaging comes from Borealis", None)
            },
        )
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    let result = rig.recall_text(rig.w1, "who supplies packaging?").await;
    let at = result
        .facts
        .iter()
        .position(|f| f.id == receipt.id)
        .expect("F1 is found through its thing");
    assert!(at < 5);
    rig.stop().await;
}

#[tokio::test]
async fn edge_link_expansion_stops_at_one_hop_criterion_4() {
    let rig = Rig::desktop().await;
    let w = rig.w1;
    let linked = |statement: &str, a: &str, b: &str| RememberInput {
        entities: vec![thing(a), thing(b)],
        links: vec![link(a, "joins", b)],
        ..input(statement, None)
    };
    let alone = |statement: &str, a: &str| RememberInput {
        entities: vec![thing(a)],
        ..input(statement, None)
    };
    // Named things stand for the scenario's letters. fAB names the first thing in its text, so the
    // top score clears the abstain threshold; the others stay unrelated to the query.
    for write in [
        linked("Alder joins Birch", "Alder", "Birch"),
        linked("Zeta two", "Birch", "Cedar"),
        linked("Zeta three", "Cedar", "Dogwood"),
    ] {
        rig.svc
            .remember_text(rig.ordin(w), write)
            .await
            .expect("remember");
    }
    let f_b = rig
        .svc
        .remember_text(rig.ordin(w), alone("Unrelated text one", "Birch"))
        .await
        .expect("fB")
        .id;
    for write in [
        alone("Unrelated text two", "Cedar"),
        alone("Unrelated text three", "Dogwood"),
    ] {
        rig.svc
            .remember_text(rig.ordin(w), write)
            .await
            .expect("fC and fD");
    }
    rig.settle().await;
    let result = rig.recall_text(w, "Alder").await;
    let found: Vec<_> = result.facts.iter().map(|f| f.id).collect();
    assert!(found.contains(&f_b), "{:?}", statements(&result));
    // The exact vector scan puts every fact of a scope this small among the candidates, so fC and
    // fD can come back on the vector list. What this scenario is about, how far the graph goes, is
    // read off the links: one hop, and no link beyond it.
    assert!(result
        .links
        .iter()
        .any(|l| l.from.name == "alder" && l.to.name == "birch"));
    assert!(!result
        .links
        .iter()
        .any(|l| l.from.name == "birch" || l.from.name == "cedar"));
    rig.stop().await;
}

#[tokio::test]
async fn edge_links_stay_in_their_workspace_criterion_6() {
    let rig = Rig::desktop().await;
    let f1 = borealis_fact(&rig).await;
    let other = rig
        .svc
        .remember_text(
            rig.ordin(rig.w2),
            RememberInput {
                entities: vec![thing("packaging")],
                ..input("Cardboard is ordered monthly", None)
            },
        )
        .await
        .expect("W2");
    rig.kept(other.id).await;
    let result = rig.recall_text(rig.w2, "who supplies packaging?").await;
    assert!(result.links.is_empty());
    assert!(!result.facts.iter().any(|f| f.id == f1));
    rig.stop().await;
}

/// The deterministic models and the storage make no call; nothing in the path has a network client.
#[tokio::test]
async fn edge_remembering_links_makes_no_outbound_call_criterion_7() {
    let rig = Rig::desktop().await;
    borealis_fact(&rig).await;
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM mem_relationship")
            .await,
        1
    );
    rig.stop().await;
}

async fn three_borealis_facts(rig: &Rig) -> Vec<MemoryFactId> {
    let mut ids = Vec::new();
    for n in 1..=3 {
        let write = RememberInput {
            entities: vec![thing("Borealis")],
            ..input(&format!("Borealis delivered late, time {n}."), None)
        };
        let receipt = rig
            .svc
            .remember_text(rig.ordin(rig.w1), write)
            .await
            .expect("remember");
        rig.kept(receipt.id).await;
        ids.push(receipt.id);
    }
    ids
}

async fn summary(rig: &Rig, made_from: Vec<MemoryFactId>) -> MemoryFactId {
    let receipt = rig
        .svc
        .remember_text(
            rig.ordin(rig.w1),
            RememberInput {
                entities: vec![thing("Borealis")],
                made_from,
                ..input(
                    "Borealis has delivered late three times this quarter.",
                    None,
                )
            },
        )
        .await
        .expect("the summary");
    rig.kept(receipt.id).await;
    receipt.id
}

#[tokio::test]
async fn happy_path_ordin_writes_a_summary_criterion_8() {
    let rig = Rig::desktop().await;
    let origins = three_borealis_facts(&rig).await;
    let s = summary(&rig, origins.clone()).await;
    let got = rig.svc.get_facts(rig.w1, vec![s]).await.expect("get");
    assert_eq!(got[0].kind, MemoryKind::Summary);
    assert_eq!(got[0].made_from_count, Some(3));
    let origins_view = rig.svc.fact_origins(s).await.expect("origins");
    let listed: Vec<_> = origins_view.items.iter().map(|i| i.fact.id).collect();
    for id in &origins {
        assert!(listed.contains(id));
    }
    assert!(origins_view
        .items
        .iter()
        .all(|i| i.state == OriginState::Current));
    let borealis = thing_named(&rig, rig.w1, "Borealis").await;
    let when = rig
        .svc
        .last_consolidated(rig.w1, borealis.id)
        .await
        .expect("last")
        .expect("a time");
    assert!(when >= rig.row(s).await.written_at);
    assert_eq!(rig.entries("memory_fact_consolidated").await.len(), 1);
    rig.stop().await;
}

#[tokio::test]
async fn edge_forgetting_an_origin_forgets_the_summary_made_from_it_sa_amem_02_replaces_criterion_8s_mark(
) {
    let rig = Rig::desktop().await;
    let origins = three_borealis_facts(&rig).await;
    let s = summary(&rig, origins.clone()).await;
    let e1 = origins[0];
    assert_eq!(
        rig.svc.forget_count(e1).await.expect("count"),
        ForgetCount { summaries: 1 }
    );
    let receipt = rig.svc.forget_fact(e1).await.expect("forget");
    assert_eq!(receipt.summaries_removed, 1);
    assert_eq!(rig.state(s).await, FactState::Forgotten);
    rig.stop().await;
}

#[tokio::test]
async fn edge_an_origin_replaced_not_forgotten_stays_listed() {
    let rig = Rig::desktop().await;
    let origins = three_borealis_facts(&rig).await;
    let s = summary(&rig, origins.clone()).await;
    let e2 = origins[1];
    let view = rig
        .svc
        .correct_fact(
            e2,
            crate::model::ObjectInput::Text("Borealis delivered on time that week.".to_owned()),
        )
        .await
        .expect("correct");
    rig.kept(view.id).await;
    rig.wait_state(e2, FactState::Replaced).await;
    let listed = rig.svc.fact_origins(s).await.expect("origins");
    let item = listed
        .items
        .iter()
        .find(|i| i.fact.id == e2)
        .expect("E2 stays listed");
    assert_eq!(item.state, OriginState::NoLongerTrue);
    rig.stop().await;
}

#[tokio::test]
async fn error_only_ordin_writes_summaries() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    let id = MemoryFactId::new();
    let problem = rig
        .provider()
        .check_arguments(
            &key("remember"),
            &json!({ "statement": "A summary.", "made_from": [id, MemoryFactId::new()] }),
        )
        .expect_err("refused");
    assert!(!problem.reason.is_empty());
    let error = rig
        .crew_call(
            "remember",
            rig.w1,
            agent,
            json!({ "statement": "A summary.", "made_from": [id, MemoryFactId::new()] }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_INVALID_ARGUMENTS");

    let write = RememberInput {
        made_from: vec![id, MemoryFactId::new()],
        ..input("A summary.", None)
    };
    assert_eq!(
        code_of(rig.svc.remember_text(rig.crew(rig.w1, agent), write).await),
        ("MEM_MADE_FROM_NOT_ALLOWED", false)
    );
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_the_things_of_a_workspace_searchable_by_other_name_criterion_9() {
    let rig = Rig::desktop().await;
    let first = RememberInput {
        entities: vec![
            ThingInput {
                name: "Borealis Ltd".to_owned(),
                other_names: vec!["Borealis".to_owned()],
            },
            thing("Thing one"),
        ],
        // ADR-118: a second thing is named by a fact of its own.
        links: vec![link("Borealis Ltd", "signs with", "Thing one")],
        ..input("Borealis Ltd signs with Thing one.", None)
    };
    let mut ids = vec![
        rig.svc
            .remember_text(rig.ordin(rig.w1), first)
            .await
            .expect("first")
            .id,
    ];
    for n in 2..=12 {
        let write = RememberInput {
            entities: vec![thing(&format!("Thing {n}"))],
            ..input(&format!("Thing {n} is on the list."), None)
        };
        ids.push(
            rig.svc
                .remember_text(rig.ordin(rig.w1), write)
                .await
                .expect("a fact")
                .id,
        );
    }
    let last = RememberInput {
        entities: vec![thing("Last thing")],
        ..input("Last thing closes the list.", None)
    };
    ids.push(
        rig.svc
            .remember_text(rig.ordin(rig.w1), last)
            .await
            .expect("a fact")
            .id,
    );
    rig.settle().await;
    let place = MemoryPlace::Workspace(rig.w1);
    let mut cursor = None;
    let mut things = Vec::new();
    loop {
        let page = rig
            .svc
            .list_things(place, None, cursor)
            .await
            .expect("things");
        things.extend(page.items);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(things.len(), 14);
    let found = rig
        .svc
        .list_things(place, Some("borealis".to_owned()), None)
        .await
        .expect("search");
    assert!(found.items.iter().any(|t| t.name == "borealis ltd"));
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_fact_corrected_twice_shows_three_versions_criterion_10() {
    let rig = Rig::desktop().await;
    let v1 = rig.fact(rig.w1, "Rates are 10 percent.").await;
    let v2 = rig
        .svc
        .correct_fact(
            v1,
            crate::model::ObjectInput::Text("Rates are 12 percent.".to_owned()),
        )
        .await
        .expect("V2")
        .id;
    rig.kept(v2).await;
    rig.wait_state(v1, FactState::Replaced).await;
    let v3 = rig
        .svc
        .correct_fact(
            v2,
            crate::model::ObjectInput::Text("Rates are 14 percent.".to_owned()),
        )
        .await
        .expect("V3")
        .id;
    rig.kept(v3).await;
    rig.wait_state(v2, FactState::Replaced).await;
    let history = rig.svc.fact_history(v3).await.expect("history");
    assert_eq!(history.versions.len(), 3);
    assert_eq!(history.versions[0].id, v1);
    assert!(history.versions[0].true_until.is_some());
    assert!(history.versions[1].true_until.is_some());
    assert_eq!(history.versions[2].kind, MemoryKind::Known);
    assert_eq!(history.versions[2].replaces, Some(v2));
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_a_fading_fact_is_kept_criterion_11() {
    let rig = Rig::desktop().await;
    let g = rig.fact(rig.w1, "Kestrel changed its logo.").await;
    let days_25 = Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 25 * 86_400_000)
        .expect("a time");
    let memory_fact_id = rig.row(g).await.memory_fact_id;
    rig.svc
        .inner
        .db
        .exec(
            q("UPDATE mem_fact SET ingested_at = $1 WHERE id = $2")
                .ts(days_25)
                .text(memory_fact_id),
        )
        .await
        .expect("age the fact");
    let page = rig.svc.list_facts(rig.w1, None).await.expect("list");
    let item = page.items.iter().find(|f| f.id == g).expect("G");
    assert!(item.fading);
    assert_eq!(item.not_used_since, Some(days_25));

    let kept = rig.svc.keep_fact(g).await.expect("keep");
    assert_eq!(kept.kind, MemoryKind::Known);
    assert!(!kept.fading);
    let entries = rig.entries("memory_fact_kept").await;
    assert_eq!(entries[0].data["fact_ref"], g.to_string());
    assert!(!entries[0].data.to_string().contains("logo"));
    assert_eq!(
        code_of(rig.svc.keep_fact(g).await),
        ("MEM_ALREADY_KEPT", false)
    );
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_what_ordin_used_criterion_12() {
    let rig = Rig::desktop().await;
    let f = borealis_fact(&rig).await;
    let g = rig
        .svc
        .remember_text(
            rig.ordin(rig.w1),
            RememberInput {
                entities: vec![thing("packaging")],
                ..input("Packaging invoices are monthly", None)
            },
        )
        .await
        .expect("g")
        .id;
    let h = rig
        .svc
        .remember_text(
            rig.ordin(rig.w1),
            RememberInput {
                entities: vec![thing("packaging")],
                ..input("Packaging samples go to Leeds", None)
            },
        )
        .await
        .expect("h")
        .id;
    rig.settle().await;
    let result = rig.recall_text(rig.w1, "Borealis").await;
    assert_eq!(result.facts.len(), 3, "{:?}", statements(&result));
    assert!(!result.links.is_empty());
    let place = MemoryPlace::Workspace(rig.w1);
    let used = rig
        .svc
        .memory_used(place, vec![result.recall_id])
        .await
        .expect("used");
    assert_eq!(used.facts.len(), 3);
    assert_eq!(used.links.len(), result.links.len());
    assert_eq!(used.gone, 0);

    // One of the two facts the link did not reach first: the link stays, and one item is gone.
    let items = crate::store::recall_items(&rig.svc.inner.db, result.recall_id)
        .await
        .expect("items");
    let reached = items
        .iter()
        .find(|i| i.item_kind == "link")
        .expect("the link")
        .fact_ref;
    let gone = if reached == g { h } else { g };
    rig.svc.forget_fact(gone).await.expect("forget");
    let again = rig.recall_text(rig.w1, "Borealis").await;
    assert!(!again.facts.iter().any(|x| x.id == gone));
    let used = rig
        .svc
        .memory_used(place, vec![result.recall_id])
        .await
        .expect("used");
    assert_eq!(used.facts.len(), 2);
    assert_eq!(used.gone, 1);
    assert!(used.facts.iter().any(|x| x.id == f));
    rig.stop().await;
}

#[tokio::test]
async fn error_a_recall_of_another_workspace() {
    let rig = Rig::desktop().await;
    rig.fact(rig.w1, "Only in the first workspace.").await;
    let r = rig.recall_text(rig.w1, "workspace").await;
    assert_eq!(
        code_of(
            rig.svc
                .memory_used(MemoryPlace::Workspace(rig.w2), vec![r.recall_id])
                .await
        ),
        ("MEM_RECALL_IDS_INVALID", false)
    );
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_remembered_during_a_run_criterion_13() {
    let rig = Rig::desktop().await;
    let a2 = AgentInstanceId::new();
    rig.fact(rig.w1, "Ordin noted the deadline.").await;
    for n in 1..=2 {
        rig.crew_call(
            "remember",
            rig.w1,
            a2,
            json!({ "statement": format!("Crew note number {n}.") }),
        )
        .await
        .expect("crew");
    }
    rig.keep_held_writes(rig.w1, 2).await;
    rig.settle().await;
    let run = rig.svc.remembered_in_run(rig.run).await.expect("run");
    assert_eq!(run.items.len(), 3);
    assert_eq!(run.items[0].by, RememberedBy::Ordin);
    for item in &run.items[1..] {
        assert_eq!(
            item.by,
            RememberedBy::CrewMember {
                agent_instance_id: a2
            }
        );
    }
    rig.stop().await;
}

/// `count` facts naming T and `links` links from T.
async fn thing_with(rig: &Rig, facts: usize, links: usize) -> MemoryThingId {
    for n in 0..facts {
        let mut write = RememberInput {
            entities: vec![thing("Tamar")],
            ..input(&format!("Tamar fact number {n}."), None)
        };
        if n < links {
            let other = format!("Peer {n}");
            write.entities.push(thing(&other));
            write.links.push(link("Tamar", "works with", &other));
        }
        rig.svc
            .remember_text(rig.ordin(rig.w1), write)
            .await
            .expect("remember");
    }
    rig.settle().await;
    thing_named(rig, rig.w1, "Tamar").await.id
}

#[tokio::test]
async fn happy_path_forget_everything_about_a_thing_criterion_14() {
    let rig = Rig::desktop().await;
    let t = thing_with(&rig, 12, 4).await;
    let place = MemoryPlace::Workspace(rig.w1);
    let count = rig.svc.count_thing(place, t.clone()).await.expect("count");
    assert_eq!((count.facts, count.links), (12, 4));
    let receipt = rig
        .svc
        .forget_thing(
            place,
            t,
            ThingCount {
                facts: 12,
                links: 4,
                summaries: 0,
            },
        )
        .await
        .expect("forget");
    assert_eq!((receipt.facts_removed, receipt.links_removed), (12, 4));
    assert_eq!(receipt.proof_digest.len(), 64);
    let result = rig.recall_text(rig.w1, "Tamar").await;
    assert!(result.facts.is_empty());
    assert!(result.links.is_empty());
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE state = 'forgotten'")
            .await,
        12
    );
    rig.stop().await;
}

/// The correction of 2026-01-21: a fact owned by someone else names the thing. The preview does not
/// count it, a teammate adding one more does not refuse the forget, and only the caller's rows end.
#[tokio::test]
async fn a_teammates_fact_is_left_out_of_the_count_and_stays_after_the_forget() {
    let rig = Rig::desktop().await;
    let t = thing_with(&rig, 2, 0).await;
    let place = MemoryPlace::Workspace(rig.w1);
    let preview = rig.svc.count_thing(place, t.clone()).await.expect("count");
    assert_eq!(preview.facts, 2);

    // A teammate's fact: another owner, shared with the team, naming the same entity.
    let own: (String,) = sqlx::query_as("SELECT id FROM mem_fact LIMIT 1")
        .fetch_one(rig.pool())
        .await
        .expect("a fact");
    let theirs = format!("fact:{}", uuid::Uuid::now_v7());
    rig.execute(&format!(
        "INSERT INTO mem_fact (id, tenant, owner_team, owner_user, content, content_text, \
         entities, memory_class, visibility, valid_from, ingested_at, confidence, salience, \
         stability) SELECT '{theirs}', tenant, owner_team, 'someone-else', content, content_text, \
         entities, memory_class, visibility, valid_from, ingested_at, confidence, salience, \
         stability FROM mem_fact WHERE id = '{}'",
        own.0
    ))
    .await;
    rig.execute(&format!(
        "INSERT INTO mem_fact_entity (fact_id, entity_id) VALUES ('{theirs}', '{}')",
        t.0
    ))
    .await;

    let after = rig.svc.count_thing(place, t.clone()).await.expect("count");
    assert_eq!(
        after, preview,
        "a teammate's write does not move the preview"
    );
    let receipt = rig
        .svc
        .forget_thing(place, t.clone(), preview)
        .await
        .expect("no wrongful refusal");
    assert_eq!(receipt.facts_removed, 2);
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE state = 'forgotten'")
            .await,
        2
    );
    let entries = rig.entries("memory_thing_forgotten").await;
    assert_eq!(entries[0].data["facts_remaining"], 1);
    assert_eq!(entries[0].data["thing_removed"], false);
    // The teammate's fact stays, and so does the thing they still name.
    assert_eq!(
        rig.count(&format!(
            "SELECT COUNT(*) AS n FROM mem_fact WHERE id = '{theirs}'"
        ))
        .await,
        1
    );
    assert_eq!(rig.count("SELECT COUNT(*) AS n FROM mem_entity").await, 1);
    rig.stop().await;
}

#[tokio::test]
async fn error_the_count_changed_before_confirming() {
    let rig = Rig::desktop().await;
    let t = thing_with(&rig, 12, 4).await;
    let more = RememberInput {
        entities: vec![thing("Tamar")],
        ..input("Tamar has one more fact.", None)
    };
    let receipt = rig
        .svc
        .remember_text(rig.ordin(rig.w1), more)
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    assert_eq!(
        code_of(
            rig.svc
                .forget_thing(
                    MemoryPlace::Workspace(rig.w1),
                    t,
                    ThingCount {
                        facts: 12,
                        links: 4,
                        summaries: 0
                    }
                )
                .await
        ),
        ("MEM_THING_COUNT_CHANGED", false)
    );
    assert_eq!(rig.count("SELECT COUNT(*) AS n FROM mem_fact").await, 13);
    rig.stop().await;
}

#[tokio::test]
async fn happy_path_same_as_criterion_15() {
    let rig = Rig::desktop().await;
    for n in 0..3 {
        let mut write = RememberInput {
            entities: vec![thing("Borealis")],
            ..input(&format!("Short name fact {n}."), None)
        };
        if n == 0 {
            write.entities.push(thing("Harbour"));
            write.links.push(link("Borealis", "uses", "Harbour"));
        }
        rig.svc
            .remember_text(rig.ordin(rig.w1), write)
            .await
            .expect("short");
    }
    for n in 0..2 {
        let write = RememberInput {
            entities: vec![thing("Northern Lights Holding")],
            ..input(&format!("Long name fact {n}."), None)
        };
        rig.svc
            .remember_text(rig.ordin(rig.w1), write)
            .await
            .expect("long");
    }
    rig.settle().await;
    let place = MemoryPlace::Workspace(rig.w1);
    let merge = thing_named(&rig, rig.w1, "Borealis").await;
    let keep = thing_named(&rig, rig.w1, "Northern Lights Holding").await;
    let view = rig
        .svc
        .same_as(place, keep.id.clone(), merge.id.clone())
        .await
        .expect("merge");
    assert_eq!((view.fact_count, view.link_count), (5, 1));
    assert!(view
        .other_names
        .iter()
        .any(|n| n.eq_ignore_ascii_case("Borealis")));
    assert_eq!(
        code_of(rig.svc.get_thing(place, merge.id).await),
        ("MEM_THING_NOT_FOUND", false)
    );
    rig.stop().await;
}

#[tokio::test]
async fn edge_journal_entries_carry_no_text_or_names_criterion_16() {
    let rig = Rig::desktop().await;
    let place = MemoryPlace::Workspace(rig.w1);
    let f = rig.fact(rig.w1, "Secret statement about Velvet.").await;
    rig.svc.keep_fact(f).await.expect("keep");
    borealis_fact(&rig).await;
    let borealis = thing_named(&rig, rig.w1, "Borealis").await;
    let packaging = thing_named(&rig, rig.w1, "packaging").await;
    let detail = rig
        .svc
        .get_thing(place, borealis.id.clone())
        .await
        .expect("thing");
    let link_id = detail.links[0].id.clone();
    let corrected = rig
        .svc
        .correct_link(
            link_id,
            "provides".to_owned(),
            borealis.id.clone(),
            packaging.id.clone(),
        )
        .await
        .expect("correct");
    assert_eq!(corrected.by, LinkBy::Person);
    rig.svc
        .forget_link(corrected.id)
        .await
        .expect("forget link");
    rig.svc
        .same_as(place, borealis.id.clone(), packaging.id)
        .await
        .expect("merge");
    let count = rig
        .svc
        .count_thing(place, borealis.id.clone())
        .await
        .expect("count");
    rig.svc
        .forget_thing(place, borealis.id, count)
        .await
        .expect("forget");
    for kind in [
        "memory_fact_kept",
        "memory_link_corrected",
        "memory_link_forgotten",
        "memory_thing_forgotten",
        "memory_things_merged",
    ] {
        let entries = rig.entries(kind).await;
        assert!(!entries.is_empty(), "{kind}");
        for entry in entries {
            let text = entry.data.to_string();
            for word in [
                "Secret",
                "Velvet",
                "Borealis",
                "packaging",
                "supplies",
                "provides",
            ] {
                assert!(!text.contains(word), "{kind}: {text}");
            }
        }
    }
    rig.stop().await;
}

#[tokio::test]
async fn error_an_agents_rules_govern_its_links_criterion_18() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    rig.svc
        .set_agent_rules(agent, rules(&["decisions"], &["contact-details"], None))
        .expect("rules");
    let error = rig
        .call(
            "remember",
            ToolCaller::Agent,
            rig.w1,
            agent,
            None,
            json!({
                "statement": "Ann's number is on file.",
                "fact_class": "contact-details",
                "entities": [{"name": "Ann"}, {"name": "file"}],
                "links": [{"from": "Ann", "kind": "has", "to": "file"}],
            }),
        )
        .await
        .expect_err("refused");
    assert_eq!(error.code, "MEM_CLASS_EXCLUDED");
    assert_eq!(rig.count("SELECT COUNT(*) AS n FROM mem_fact").await, 0);
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM mem_relationship")
            .await,
        0
    );
    rig.stop().await;
}

fn grant(caller: ToolCaller, agent: AgentInstanceId, rig: &Rig) -> SessionGrant {
    SessionGrant {
        run_id: rig.run,
        workspace_id: rig.w1,
        agent_instance_id: agent,
        harness: HarnessKind::ClaudeCode,
        profile: ProgramProfile::Desktop,
        caller,
        scope_id: None,
        coding_task_id: None,
        only_servers: None,
    }
}

#[tokio::test]
async fn edge_an_agent_without_memory_rules_gets_no_memory() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    let provider = rig.provider();
    let offered = provider
        .offer(&grant(ToolCaller::Agent, agent, &rig))
        .await
        .expect("offer");
    assert!(offered.is_empty());
    rig.svc
        .set_agent_rules(agent, rules(&[], &[], None))
        .expect("rules");
    let offered = provider
        .offer(&grant(ToolCaller::Agent, agent, &rig))
        .await
        .expect("offer");
    assert_eq!(offered.len(), 2);
    rig.svc.clear_agent_rules(agent);
    assert!(provider
        .offer(&grant(ToolCaller::Agent, agent, &rig))
        .await
        .expect("offer")
        .is_empty());
    assert!(provider
        .offer(&grant(ToolCaller::Ordin, agent, &rig))
        .await
        .expect("offer")
        .is_empty());
    rig.stop().await;
}

/// Three existing facts stand for the scenario's 300: rows with `caller_kind` NULL and entities
/// whose name rows are absent, as before the migration.
#[tokio::test]
async fn edge_an_installation_from_before_this_change_criterion_19() {
    let rig = Rig::desktop().await;
    for n in 0..3 {
        let write = RememberInput {
            entities: vec![thing(&format!("Old thing {n}"))],
            ..input(&format!("Old fact number {n}."), None)
        };
        rig.svc
            .remember_text(rig.ordin(rig.w1), write)
            .await
            .expect("remember");
    }
    rig.settle().await;
    rig.execute("UPDATE memory_fact_refs SET caller_kind = NULL")
        .await;
    rig.execute("DELETE FROM mem_entity_name").await;
    rig.execute("UPDATE mem_maintenance_state SET names_backfilled_at = NULL")
        .await;
    let inner = rig.svc.inner.clone();
    let token = tokio_util::sync::CancellationToken::new();
    inner.run_backfill_for_test(token).await;
    let page = rig.svc.list_facts(rig.w1, None).await.expect("list");
    assert_eq!(page.items.len(), 3);
    assert!(page.items.iter().all(|f| !f.things.is_empty()));
    let things = rig
        .svc
        .list_things(MemoryPlace::Workspace(rig.w1), None, None)
        .await
        .expect("things");
    assert_eq!(things.items.len(), 3);
    assert!(things.items.iter().all(|t| t.link_count == 0));
    let result = rig.recall_text(rig.w1, "Old thing 1").await;
    assert!(!result.facts.is_empty());
    rig.stop().await;
}

/// The page's title and links stay in the memory for recall (ADR-118 item 8), but a thing named only
/// by passages is not listed and its page link is on no thing's page (MEM-TR-8). Replaces the ADR-111
/// scenario "a page's title and links become a thing and a link".
// spec: ordin-desktop/components/MemoryService#MEM-TR-8
#[tokio::test]
async fn edge_a_pages_title_and_links_are_not_listed_as_things() {
    let rig = Rig::desktop().await;
    let a = KnowledgePageId::new();
    let b = KnowledgePageId::new();
    let place = MemoryPlace::Workspace(rig.w1);
    let linked = PageGraph {
        title: "Supplier Borealis".to_owned(),
        other_names: vec!["Borealis".to_owned()],
        links_to: vec![(b, "Packaging standards".to_owned())],
        standing: PageStanding::default(),
    };
    rig.page(rig.w1, a, &[("Terms", "Net thirty days.")], "1", linked)
        .await;
    let things = rig
        .svc
        .list_things(place, Some("Supplier Borealis".to_owned()), None)
        .await
        .expect("things");
    assert!(things.items.is_empty(), "{:?}", things.items);
    let stored = rig
        .count("SELECT COUNT(*) AS n FROM mem_entity WHERE canonical_name = 'supplier borealis'")
        .await;
    assert_eq!(stored, 1, "the page's thing stays for recall");
    rig.stop().await;
}

#[tokio::test]
async fn edge_a_page_backed_fact_outranks_an_equal_document_fact_criterion_22() {
    let rig = Rig::desktop().await;
    let text = "Payment terms are thirty days.";
    let mut document = input(text, None);
    document.source = Some(FactSource {
        origin_ref: "SharePoint — Terms.pdf".to_owned(),
        version: "1".to_owned(),
    });
    let receipt = rig
        .svc
        .remember_text(rig.ordin(rig.w1), document)
        .await
        .expect("remember");
    rig.kept(receipt.id).await;
    let p = KnowledgePageId::new();
    rig.page(rig.w1, p, &[("", text)], "1", graph("Terms"))
        .await;
    let result = rig.recall_text(rig.w1, "payment terms").await;
    assert_eq!(result.facts[0].page_id, Some(p));
    assert_eq!(result.facts[1].id, receipt.id);
    rig.stop().await;
}
