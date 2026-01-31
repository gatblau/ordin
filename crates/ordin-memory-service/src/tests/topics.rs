//! The scenarios and Error Table rows of the amendment "Topics of things" (MEM-TOP-1 to MEM-TOP-7,
//! amended January 2026 (user decision; Approved by the user January 2026)), each a test of the
//! scenario's name.

use ordin_types::{MemoryFactId, MemoryThingId, WorkspaceId};
use serde_json::{json, Value};

use super::support::*;
use crate::model::*;

fn input(items: Vec<Value>) -> RememberFactsInput {
    RememberFactsInput {
        source: Some(FactSourceInput::Conversation),
        fact_class: None,
        facts: items
            .into_iter()
            .map(|v| serde_json::from_value(v).expect("a fact"))
            .collect(),
    }
}

fn employs(subject: &str, topic: Option<&str>) -> Value {
    let mut subject = json!({ "name": subject });
    if let Some(topic) = topic {
        subject["topic"] = json!(topic);
    }
    json!({
        "subject": subject,
        "predicate": "numberOfEmployees",
        "object": { "number": { "value": "240", "unit": "people" } }
    })
}

impl Rig {
    async fn remembered(&self, w: WorkspaceId, items: Vec<Value>) -> Vec<MemoryFactId> {
        let receipt = self
            .svc
            .remember_facts(self.ordin(w), input(items))
            .await
            .expect("remember");
        let mut ids = Vec::new();
        for item in receipt.items {
            self.kept(item.id).await;
            ids.push(item.id);
        }
        ids
    }

    async fn thing_id(&self, w: WorkspaceId, name: &str) -> MemoryThingId {
        self.svc
            .list_things(MemoryPlace::Workspace(w), Some(name.to_owned()), None)
            .await
            .expect("things")
            .items
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
            .expect("the thing")
            .id
    }

    async fn topic_rows(&self) -> i64 {
        self.count("SELECT COUNT(*) AS n FROM memory_thing_topics")
            .await
    }
}

// Scenario: Happy path — Ordin gives a new thing a topic
// spec: ordin-desktop/components/MemoryService#MEM-TOP-3
// spec: ordin-desktop/components/MemoryService#MEM-TOP-4
#[tokio::test]
async fn happy_path_ordin_gives_a_new_thing_a_topic() {
    let rig = Rig::desktop().await;
    let place = MemoryPlace::Workspace(rig.w1);
    rig.remembered(rig.w1, vec![employs("Kestrel", Some("Suppliers"))])
        .await;
    let view = rig.svc.thing_topics(place).await.expect("topics");
    let kestrel = rig.thing_id(rig.w1, "Kestrel").await;
    assert_eq!(view.topics, vec!["Suppliers".to_owned()]);
    assert_eq!(view.things.len(), 1);
    assert_eq!(view.things[0].thing_id, kestrel);
    assert_eq!(view.things[0].set_by, TopicSetBy::Ordin);
    // Another workspace sees none of it.
    let other = rig
        .svc
        .thing_topics(MemoryPlace::Workspace(rig.w2))
        .await
        .expect("topics");
    assert!(other.things.is_empty());
    rig.stop().await;
}

// Scenario: Edge — a second topic never replaces the first, and a topic is reused as written
// spec: ordin-desktop/components/MemoryService#MEM-TOP-3
// spec: ordin-desktop/components/MemoryService#MEM-TOP-5
#[tokio::test]
async fn edge_a_second_topic_never_replaces_the_first() {
    let rig = Rig::desktop().await;
    let place = MemoryPlace::Workspace(rig.w1);
    rig.remembered(rig.w1, vec![employs("Kestrel", Some("Suppliers"))])
        .await;
    rig.svc.thing_topics(place).await.expect("settle");
    rig.remembered(
        rig.w1,
        vec![
            json!({
                "subject": { "name": "Kestrel", "topic": "People" },
                "predicate": "foundingDate",
                "object": { "date": "2001" }
            }),
            employs("Borealis", Some("  suppliers ")),
        ],
    )
    .await;
    let view = rig.svc.thing_topics(place).await.expect("topics");
    assert_eq!(view.topics, vec!["Suppliers".to_owned()]);
    assert_eq!(view.things.len(), 2);
    assert!(view.things.iter().all(|t| t.topic == "Suppliers"));
    rig.stop().await;
}

// Scenario: Happy path — the person changes and clears a thing's topic
// spec: ordin-desktop/components/MemoryService#MEM-TOP-5
#[tokio::test]
async fn happy_path_the_person_changes_and_clears_a_topic() {
    let rig = Rig::desktop().await;
    let place = MemoryPlace::Workspace(rig.w1);
    rig.remembered(rig.w1, vec![employs("Kestrel", None)]).await;
    let kestrel = rig.thing_id(rig.w1, "Kestrel").await;
    let set = rig
        .svc
        .set_thing_topic(place, kestrel.clone(), Some(" Funding ".to_owned()))
        .await
        .expect("set");
    assert_eq!(set.things.len(), 1);
    assert_eq!(set.things[0].topic, "Funding");
    assert_eq!(set.things[0].set_by, TopicSetBy::Person);
    let changed = rig
        .svc
        .set_thing_topic(place, kestrel.clone(), Some("Strategy".to_owned()))
        .await
        .expect("change");
    assert_eq!(changed.topics, vec!["Strategy".to_owned()]);
    let cleared = rig
        .svc
        .set_thing_topic(place, kestrel, None)
        .await
        .expect("clear");
    assert!(cleared.things.is_empty());
    assert_eq!(rig.entries("memory_topic_set").await.len(), 3);
    rig.stop().await;
}

// Error Table: a remembered fact whose subject's topic is out of bounds
// spec: ordin-desktop/components/MemoryService#MEM-TOP-2
#[tokio::test]
async fn error_mem_topic_invalid_on_remember() {
    let rig = Rig::desktop().await;
    for topic in ["", "   ", &"t".repeat(41), "Fund\u{7}ing"] {
        let err = rig
            .svc
            .remember_facts(
                rig.ordin(rig.w1),
                input(vec![employs("Kestrel", Some(topic))]),
            )
            .await
            .expect_err("refused");
        assert_eq!(err.code(), "MEM_TOPIC_INVALID", "{topic:?}");
    }
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs")
            .await,
        0
    );
    rig.stop().await;
}

// Error Table: a remembered fact whose subject's topic holds personal data
// spec: ordin-desktop/components/MemoryService#MEM-TOP-2
#[tokio::test]
async fn error_mem_topic_refused_on_remember() {
    let rig = Rig::desktop().await;
    let err = rig
        .svc
        .remember_facts(
            rig.ordin(rig.w1),
            input(vec![employs("Kestrel", Some("ana@example.com"))]),
        )
        .await
        .expect_err("refused");
    assert_eq!(err.code(), "MEM_TOPIC_REFUSED");
    assert_eq!(
        rig.count("SELECT COUNT(*) AS n FROM memory_fact_refs")
            .await,
        0
    );
    rig.stop().await;
}

// Error Table: the person's topic out of bounds, holding personal data, or for no such thing
// spec: ordin-desktop/components/MemoryService#MEM-TOP-5
#[tokio::test]
async fn error_the_persons_topic_is_checked() {
    let rig = Rig::desktop().await;
    let place = MemoryPlace::Workspace(rig.w1);
    rig.remembered(rig.w1, vec![employs("Kestrel", None)]).await;
    let kestrel = rig.thing_id(rig.w1, "Kestrel").await;
    let invalid = rig
        .svc
        .set_thing_topic(place, kestrel.clone(), Some("t".repeat(41)))
        .await
        .expect_err("invalid");
    assert_eq!(invalid.code(), "MEM_TOPIC_INVALID");
    let refused = rig
        .svc
        .set_thing_topic(place, kestrel, Some("ana@example.com".to_owned()))
        .await
        .expect_err("refused");
    assert_eq!(refused.code(), "MEM_TOPIC_REFUSED");
    let missing = rig
        .svc
        .set_thing_topic(
            place,
            MemoryThingId("entity:0192a0f1-3333-7444-8555-666677778888".to_owned()),
            Some("Funding".to_owned()),
        )
        .await
        .expect_err("missing");
    assert_eq!(missing.code(), "MEM_THING_NOT_FOUND");
    assert_eq!(rig.topic_rows().await, 0);
    rig.stop().await;
}

// Edge — a topic of 40 characters is kept (MEM-TOP-2's bound)
// spec: ordin-desktop/components/MemoryService#MEM-TOP-2
#[test]
fn edge_the_topic_bounds() {
    assert!(crate::topics::topic_ok(&"t".repeat(40)));
    assert!(!crate::topics::topic_ok(&"t".repeat(41)));
    assert!(crate::topics::topic_ok("Funding"));
    assert!(!crate::topics::topic_ok(" "));
}
