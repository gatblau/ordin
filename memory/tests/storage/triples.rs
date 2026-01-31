//! Triples, values, conflicts and passages (ADR-022; ordin change M-10): the memory-store amendment's
//! Gherkin scenarios at the store level, and the write-pipeline, service-layer, retrieval and
//! http-edge scenarios through the Service — every one run against SurrealDB, SQLite and PostgreSQL
//! (Playbook Step 6h: "every C1 scenario also against the SQL storage on SQLite and PostgreSQL").

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use chrono::Utc;
use memory::types::domain::{Entity, Fact, MemoryClass, ObjectKind, TripleKey, Visibility};
use memory::types::ports::{LinkRole, StoragePort, StoreError};
use memory::types::scope::{ScopeContext, ScopeRef};

use crate::suite::{at, ctx, maint, scope_ref};
use crate::Harness;

// --- Fixtures ----------------------------------------------------------------------------------------

const T: &str = "acme";

fn owner(team: &str) -> ScopeRef {
    scope_ref(T, Some(team), &format!("u-{team}"))
}

fn w_ctx() -> ScopeContext {
    ctx(T, "u-ws:W", &["ws:W"])
}

fn ent(id: &str, name: &str, team: &str) -> Entity {
    Entity {
        private_aliases: vec![],
        alias_folders: vec![],
        id: id.into(),
        canonical_name: name.into(),
        aliases: vec![],
        owner: owner(team),
        collection: String::new(),
    }
}

/// A live team-shared fact of `team` naming `entities`, with `content` and no triple.
fn fact(id: &str, team: &str, entities: &[&str], content: serde_json::Value) -> Fact {
    Fact {
        id: id.into(),
        content,
        entities: entities.iter().map(|e| e.to_string()).collect(),
        source_id: None,
        memory_class: MemoryClass::Semantic,
        fact_class: None,
        visibility: Visibility::TeamShared,
        owner: owner(team),
        valid_from: at("2026-01-01T00:00:00.000Z"),
        valid_to: None,
        ingested_at: at("2026-01-01T00:00:00.000Z"),
        confidence: 0.9,
        salience: 0.5,
        stability: 1.0,
        pii_review: false,
        supersedes: None,
        superseded_by: None,
        derived_from: vec![],
        last_recalled_at: None,
        collection: String::new(),
        single_valued: false,
        changed: false,
        triple_key: None,
    }
}

fn manager_content(subject: &str, object: &str) -> serde_json::Value {
    serde_json::json!({
        "subject": subject, "predicate": "accountManager",
        "object": {"kind": "entity", "name": object}
    })
}

/// A live triple fact `<subject entity> accountManager <object entity>` of `team`.
fn triple_fact(id: &str, team: &str, subject: (&str, &str), object: (&str, &str)) -> Fact {
    let mut f = fact(
        id,
        team,
        &[subject.0, object.0],
        manager_content(subject.1, object.1),
    );
    f.single_valued = true;
    f.triple_key = Some(TripleKey {
        subject_entity: subject.0.into(),
        predicate: "accountManager".into(),
        object_kind: ObjectKind::Entity,
        object_value: object.0.into(),
        object_unit: None,
        object_entity: Some(object.0.into()),
        author: None,
    });
    f
}

const K: (&str, &str) = ("entity:kestrel", "Kestrel");
const TOM: (&str, &str) = ("entity:tom", "Tom Reid");
const ANA: (&str, &str) = ("entity:ana", "Ana Silva");

async fn seed_entities(s: &Arc<dyn StoragePort>, team: &str) {
    for (id, name) in [K, TOM, ANA] {
        s.put_entity(&ent(id, name, team)).await.unwrap();
    }
}

fn ids(facts: &[Fact]) -> Vec<String> {
    let mut v: Vec<String> = facts.iter().map(|f| f.id.clone()).collect();
    v.sort();
    v
}

async fn with_of(s: &Arc<dyn StoragePort>, c: &ScopeContext, id: &str) -> Vec<String> {
    s.conflicts_of(c, &[id.to_string()])
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.with)
        .collect()
}

fn validation(e: StoreError) -> String {
    match e {
        StoreError::Validation(m) => m,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

// --- Memory Store scenarios (memory-store.md, amendment ADR-022) -------------------------------------

/// Happy path — a triple's parts are stored and found by one lookup; its text splits the predicate.
// spec: components/memory-store#SA-TRIPLE-03
pub async fn a_triples_parts_are_stored_and_found_by_one_lookup(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W").await;
    let f = triple_fact("fact:f", "ws:W", K, TOM);
    s.put_fact(&f).await.unwrap();
    let got = s
        .live_same_subject_predicate(&w_ctx(), &owner("ws:W"), K.0, "accountManager", 64)
        .await
        .unwrap();
    assert_eq!(ids(&got), vec!["fact:f".to_string()]);
    assert_eq!(got[0].triple_key, f.triple_key, "the parts read back");
    assert!(got[0].single_valued);
    let text = crate::service_suite::column_values(
        &h,
        "SELECT content_text FROM mem_fact WHERE id = 'fact:f'",
        "USE NS acme DB memory; SELECT content_text AS v FROM fact WHERE id = fact:f",
    )
    .await;
    assert_eq!(text, vec!["Kestrel account Manager Tom Reid".to_string()]);
    // The triple's fields are never serialised to a caller.
    let json = serde_json::to_value(&got[0]).unwrap();
    assert!(json.get("triple_key").is_none() && json.get("subject_entity").is_none());
}

/// Happy path — a conflict is recorded and read, the pair ordered by id.
pub async fn a_conflict_is_recorded_and_read(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W").await;
    s.put_fact(&triple_fact("fact:f1", "ws:W", K, TOM))
        .await
        .unwrap();
    s.put_fact(&triple_fact("fact:f2", "ws:W", K, ANA))
        .await
        .unwrap();
    let now = Utc::now();
    assert!(s
        .record_conflict(&w_ctx(), "fact:f2", "fact:f1", now)
        .await
        .unwrap());
    // An upsert: recording the pair again writes no second row.
    assert!(s
        .record_conflict(&w_ctx(), "fact:f1", "fact:f2", now)
        .await
        .unwrap());
    let rows = s
        .conflicts_of(&w_ctx(), &["fact:f1".to_string()])
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].fact.as_str(), rows[0].with.as_str()),
        ("fact:f1", "fact:f2")
    );
    let pairs = crate::service_suite::column_values(
        &h,
        "SELECT fact_a || '|' || fact_b FROM mem_fact_conflict",
        "USE NS acme DB memory; SELECT string::concat(fact_a, '|', fact_b) AS v FROM fact_conflict",
    )
    .await;
    assert_eq!(pairs, vec!["fact:f1|fact:f2".to_string()]);
}

/// Edge — the lookup reaches every visibility and collection of the owner (ADR-022 (4)).
pub async fn the_lookup_reaches_every_visibility_and_collection(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W").await;
    let mut f1 = triple_fact("fact:f1", "ws:W", K, TOM);
    f1.visibility = Visibility::TenantShared;
    let mut f2 = triple_fact("fact:f2", "ws:W", K, ANA);
    f2.collection = "team:folder-1".into();
    s.put_fact(&f1).await.unwrap();
    s.put_fact(&f2).await.unwrap();
    // A different predicate and an ended fact are not returned.
    let mut other = triple_fact("fact:f3", "ws:W", K, ANA);
    other.triple_key.as_mut().unwrap().predicate = "worksFor".into();
    s.put_fact(&other).await.unwrap();
    let mut ended = triple_fact("fact:f4", "ws:W", K, ANA);
    ended.valid_to = Some(at("2026-01-02T00:00:00.000Z"));
    s.put_fact(&ended).await.unwrap();
    let got = s
        .live_same_subject_predicate(&w_ctx(), &owner("ws:W"), K.0, "accountManager", 64)
        .await
        .unwrap();
    assert_eq!(
        ids(&got),
        vec!["fact:f1".to_string(), "fact:f2".to_string()]
    );
}

/// Error — a conflict across owners is refused, and nothing is written.
pub async fn a_conflict_across_owners_is_refused(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W1").await;
    s.put_fact(&triple_fact("fact:f1", "ws:W1", K, TOM))
        .await
        .unwrap();
    s.put_fact(&triple_fact("fact:f2", "ws:W2", K, ANA))
        .await
        .unwrap();
    let e = s
        .record_conflict(&maint(T), "fact:f1", "fact:f2", Utc::now())
        .await
        .err()
        .unwrap();
    assert_eq!(validation(e), "conflict owner");
    assert!(s
        .conflicts_of(&maint(T), &["fact:f1".to_string()])
        .await
        .unwrap()
        .is_empty());
}

/// Edge — a conflict goes when one side ends (end_validity, supersede, hard_delete).
pub async fn a_conflict_goes_when_one_side_ends(h: Harness) {
    let s = &h.storage;
    let c = w_ctx();
    seed_entities(s, "ws:W").await;
    for (id, obj) in [
        ("fact:f1", TOM),
        ("fact:f2", ANA),
        ("fact:f3", TOM),
        ("fact:f4", ANA),
    ] {
        s.put_fact(&triple_fact(id, "ws:W", K, obj)).await.unwrap();
    }
    let now = Utc::now();
    for (a, b) in [
        ("fact:f1", "fact:f2"),
        ("fact:f3", "fact:f4"),
        ("fact:f2", "fact:f3"),
    ] {
        assert!(s.record_conflict(&c, a, b, now).await.unwrap());
    }
    s.end_validity(&c, "fact:f1", now).await.unwrap();
    assert!(with_of(s, &c, "fact:f1").await.is_empty());
    assert_eq!(with_of(s, &c, "fact:f2").await, vec!["fact:f3".to_string()]);
    s.put_fact(&triple_fact("fact:f5", "ws:W", K, ANA))
        .await
        .unwrap();
    s.supersede(&c, "fact:f3", "fact:f5", now).await.unwrap();
    assert!(with_of(s, &c, "fact:f2").await.is_empty());
    assert!(with_of(s, &c, "fact:f4").await.is_empty());
    assert!(s
        .record_conflict(&c, "fact:f4", "fact:f5", now)
        .await
        .unwrap());
    s.hard_delete(&c, "fact:f5").await.unwrap();
    assert!(with_of(s, &c, "fact:f4").await.is_empty());
}

/// Edge — a conflict with an ended or absent fact is not written.
pub async fn a_conflict_with_an_ended_fact_is_not_written(h: Harness) {
    let s = &h.storage;
    let c = w_ctx();
    seed_entities(s, "ws:W").await;
    s.put_fact(&triple_fact("fact:f1", "ws:W", K, TOM))
        .await
        .unwrap();
    let mut f3 = triple_fact("fact:f3", "ws:W", K, ANA);
    f3.valid_to = Some(at("2026-01-02T00:00:00.000Z"));
    s.put_fact(&f3).await.unwrap();
    assert!(!s
        .record_conflict(&c, "fact:f1", "fact:f3", Utc::now())
        .await
        .unwrap());
    assert!(!s
        .record_conflict(&c, "fact:f1", "fact:missing", Utc::now())
        .await
        .unwrap());
    assert!(with_of(s, &c, "fact:f1").await.is_empty());
}

/// Happy path — the last fact takes its entity and a person's link with it (SA-TRIPLE-10).
// spec: components/memory-store#SA-TRIPLE-10
pub async fn the_last_fact_takes_its_entity_and_a_persons_link(h: Harness) {
    let s = &h.storage;
    let c = w_ctx();
    s.put_entity(&ent("entity:p", "Pat", "ws:W")).await.unwrap();
    s.put_entity(&ent(K.0, K.1, "ws:W")).await.unwrap();
    s.put_fact(&fact(
        "fact:f",
        "ws:W",
        &["entity:p"],
        serde_json::json!({"statement": "Pat"}),
    ))
    .await
    .unwrap();
    // K is named by another fact, so it stays; R is a person's correction with no asserting fact.
    s.put_fact(&fact(
        "fact:k",
        "ws:W",
        &[K.0],
        serde_json::json!({"statement": "Kestrel"}),
    ))
    .await
    .unwrap();
    let r = memory::types::domain::Relationship {
        owner_only: false,
        author: None,
        id: "relationship:r".into(),
        kind: "knows".into(),
        from: "entity:p".into(),
        to: K.0.into(),
        valid_from: at("2026-01-01T00:00:00.000Z"),
        valid_to: None,
        ingested_at: at("2026-01-01T00:00:00.000Z"),
        confidence: 1.0,
        source_id: None,
        owner: owner("ws:W"),
        collection: String::new(),
        replaces: None,
    };

    s.put_relationship(&r).await.unwrap();
    let proof = s.hard_delete(&c, "fact:f").await.unwrap();
    assert_eq!(proof.entities_removed, vec!["entity:p".to_string()]);
    assert_eq!(
        proof.relationships_removed,
        vec!["relationship:r".to_string()]
    );
    assert!(s.get_entity(&maint(T), "entity:p").await.unwrap().is_none());
    assert!(s
        .get_relationship(&maint(T), "relationship:r")
        .await
        .unwrap()
        .is_none());
    assert!(s.get_entity(&maint(T), K.0).await.unwrap().is_some());
    // The digest covers both: it is the hash of every removed id, sorted.
    let mut all = [
        "fact:f".to_string(),
        "entity:p".to_string(),
        "relationship:r".to_string(),
    ];
    all.sort();
    let digest = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(all.join("\n").as_bytes());
        format!("{:x}", h.finalize())
    };
    assert_eq!(proof.digest, digest);
}

/// Edge — an entity another fact names stays; so does one an asserted link or another owner's link
/// touches.
pub async fn an_entity_another_fact_names_stays(h: Harness) {
    let s = &h.storage;
    let c = w_ctx();
    s.put_entity(&ent(K.0, K.1, "ws:W")).await.unwrap();
    s.put_entity(&ent("entity:q", "Quill", "ws:W"))
        .await
        .unwrap();
    s.put_fact(&fact(
        "fact:f",
        "ws:W",
        &[K.0, "entity:q"],
        serde_json::json!({"statement": "f"}),
    ))
    .await
    .unwrap();
    s.put_fact(&fact(
        "fact:g",
        "ws:W",
        &[K.0],
        serde_json::json!({"statement": "g"}),
    ))
    .await
    .unwrap();
    // `entity:q` is touched by a link another fact asserts.
    s.put_fact(&fact(
        "fact:h",
        "ws:W",
        &[K.0],
        serde_json::json!({"statement": "h"}),
    ))
    .await
    .unwrap();
    let r = memory::types::domain::Relationship {
        owner_only: false,
        author: None,
        id: "relationship:asserted".into(),
        kind: "knows".into(),
        from: "entity:q".into(),
        to: K.0.into(),
        valid_from: at("2026-01-01T00:00:00.000Z"),
        valid_to: None,
        ingested_at: at("2026-01-01T00:00:00.000Z"),
        confidence: 1.0,
        source_id: None,
        owner: owner("ws:W"),
        collection: String::new(),
        replaces: None,
    };
    s.put_relationship(&r).await.unwrap();
    s.link_fact(&maint(T), &r.id, "fact:h", LinkRole::Asserted)
        .await
        .unwrap();
    let proof = s.hard_delete(&c, "fact:f").await.unwrap();
    assert!(
        proof.entities_removed.is_empty(),
        "{:?}",
        proof.entities_removed
    );
    assert!(
        proof.relationships_removed.is_empty(),
        "{:?}",
        proof.relationships_removed
    );
}

/// Edge — earlier facts keep their shape: no triple fields, and the contradiction scan reads them.
// spec: components/memory-store#SA-TRIPLE-12
pub async fn earlier_facts_keep_their_shape(h: Harness) {
    let s = &h.storage;
    s.put_entity(&ent("entity:b", "Borealis", "ws:W"))
        .await
        .unwrap();
    let content =
        serde_json::json!({"statement": "Borealis supplies packaging", "subject": "Borealis"});
    s.put_fact(&fact("fact:old", "ws:W", &["entity:b"], content.clone()))
        .await
        .unwrap();
    s.put_fact(&fact(
        "fact:old2",
        "ws:W",
        &["entity:b"],
        serde_json::json!({"statement": "b"}),
    ))
    .await
    .unwrap();
    // A triple fact sharing the entity is compared at the write, never by the scan.
    let mut t = triple_fact("fact:new", "ws:W", ("entity:b", "Borealis"), TOM);
    t.entities = vec!["entity:b".into()];
    s.put_entity(&ent(TOM.0, TOM.1, "ws:W")).await.unwrap();
    s.put_fact(&t).await.unwrap();
    let back = s.get_fact(&maint(T), "fact:old").await.unwrap().unwrap();
    assert_eq!(back.content, content);
    assert!(back.triple_key.is_none() && !back.single_valued);
    let pairs = s
        .scan_contradiction_candidates(&maint(T), 10)
        .await
        .unwrap();
    assert!(pairs
        .iter()
        .any(|(a, b)| [a.id.as_str(), b.id.as_str()].contains(&"fact:old")));
    assert!(pairs
        .iter()
        .all(|(a, b)| a.id != "fact:new" && b.id != "fact:new"));
}

/// Edge — a passage stores no triple and the lookup never returns it.
pub async fn a_passage_stores_no_triple(h: Harness) {
    let s = &h.storage;
    s.put_entity(&ent(K.0, K.1, "ws:W")).await.unwrap();
    let mut p = fact(
        "fact:p",
        "ws:W",
        &[K.0],
        serde_json::json!({"subject": "Kestrel", "predicate": "accountManager",
                           "object": {"kind": "entity", "name": "Tom Reid"}}),
    );
    p.fact_class = Some("passage".into());
    s.put_fact(&p).await.unwrap();
    assert!(s
        .live_same_subject_predicate(&w_ctx(), &owner("ws:W"), K.0, "accountManager", 64)
        .await
        .unwrap()
        .is_empty());
    let back = s.get_fact(&maint(T), "fact:p").await.unwrap().unwrap();
    assert!(back.triple_key.is_none());
}

/// Error rows — a predicate without a subject entity, an entity object without its id, a limit or an
/// id list out of range, and the same fact twice.
pub async fn the_triple_store_refuses_what_its_error_table_names(h: Harness) {
    let s = &h.storage;
    let mut f = triple_fact("fact:f", "ws:W", K, TOM);
    f.triple_key.as_mut().unwrap().subject_entity = String::new();
    assert_eq!(validation(s.put_fact(&f).await.err().unwrap()), "triple");
    let mut f = triple_fact("fact:f", "ws:W", K, TOM);
    f.triple_key.as_mut().unwrap().object_entity = None;
    assert_eq!(validation(s.put_fact(&f).await.err().unwrap()), "triple");
    assert!(s.get_fact(&maint(T), "fact:f").await.unwrap().is_none());
    for limit in [0, 65] {
        let e = s
            .live_same_subject_predicate(&w_ctx(), &owner("ws:W"), K.0, "p", limit)
            .await
            .err()
            .unwrap();
        assert_eq!(validation(e), "limit out of range");
    }
    for n in [0usize, 101] {
        let many: Vec<String> = (0..n).map(|i| format!("fact:{i}")).collect();
        let e = s.conflicts_of(&w_ctx(), &many).await.err().unwrap();
        assert_eq!(validation(e), "ids out of range");
    }
    let e = s
        .record_conflict(&w_ctx(), "fact:f", "fact:f", Utc::now())
        .await
        .err()
        .unwrap();
    assert_eq!(validation(e), "conflict");
}

/// Security — `conflicts_of` returns only pairs whose other fact the caller can read.
pub async fn a_conflict_hides_a_fact_the_reader_cannot_read(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W").await;
    s.put_fact(&triple_fact("fact:f1", "ws:W", K, TOM))
        .await
        .unwrap();
    let mut private = triple_fact("fact:f2", "ws:W", K, ANA);
    private.visibility = Visibility::UserPrivate;
    s.put_fact(&private).await.unwrap();
    assert!(s
        .record_conflict(&w_ctx(), "fact:f1", "fact:f2", Utc::now())
        .await
        .unwrap());
    assert_eq!(
        with_of(s, &w_ctx(), "fact:f1").await,
        vec!["fact:f2".to_string()]
    );
    let mate = ctx(T, "u-mate", &["ws:W"]);
    assert!(with_of(s, &mate, "fact:f1").await.is_empty());
}

/// Erasure — the conflict rows of a scope's facts go with them, uncounted (step 20s′), and an
/// entity's erasure takes its facts' rows (20i′).
pub async fn erasures_take_the_conflict_rows(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W").await;
    s.put_fact(&triple_fact("fact:f1", "ws:W", K, TOM))
        .await
        .unwrap();
    s.put_fact(&triple_fact("fact:f2", "ws:W", K, ANA))
        .await
        .unwrap();
    assert!(s
        .record_conflict(&w_ctx(), "fact:f1", "fact:f2", Utc::now())
        .await
        .unwrap());
    let proof = s.erase_scope(T, "ws:W").await.unwrap();
    assert!(!proof.removed.contains_key("fact_conflict"));
    assert!(!proof.removed.contains_key("mem_fact_conflict"));
    let left = crate::service_suite::column_values(
        &h,
        "SELECT fact_a FROM mem_fact_conflict",
        "USE NS acme DB memory; SELECT fact_a AS v FROM fact_conflict",
    )
    .await;
    assert!(left.is_empty(), "{left:?}");

    // erase_entity: the facts it deletes take their rows.
    seed_entities(s, "ws:V").await;
    let v = ctx(T, "u-ws:V", &["ws:V"]);
    s.put_fact(&triple_fact("fact:v1", "ws:V", K, TOM))
        .await
        .unwrap();
    s.put_fact(&triple_fact("fact:v2", "ws:V", K, ANA))
        .await
        .unwrap();
    assert!(s
        .record_conflict(&v, "fact:v1", "fact:v2", Utc::now())
        .await
        .unwrap());
    s.erase_entity(&v, TOM.0).await.unwrap();
    assert!(with_of(s, &v, "fact:v2").await.is_empty());
}

/// Merge — a triple's subject and entity object follow a merge of their entity, so the lookup still
/// finds the fact under the kept entity.
pub async fn a_triple_follows_an_entity_merge(h: Harness) {
    let s = &h.storage;
    seed_entities(s, "ws:W").await;
    s.put_entity(&ent("entity:kestrel2", "Kestrel Ltd", "ws:W"))
        .await
        .unwrap();
    s.put_fact(&triple_fact(
        "fact:f1",
        "ws:W",
        ("entity:kestrel2", "Kestrel Ltd"),
        TOM,
    ))
    .await
    .unwrap();
    s.merge_entities(&w_ctx(), K.0, "entity:kestrel2")
        .await
        .unwrap();
    let got = s
        .live_same_subject_predicate(&w_ctx(), &owner("ws:W"), K.0, "accountManager", 64)
        .await
        .unwrap();
    assert_eq!(ids(&got), vec!["fact:f1".to_string()]);
}

// --- Through the Service: write pipeline, service layer, retrieval -----------------------------------

pub mod service {
    use super::*;
    use crate::service_suite::{
        a_ctx, a_owner, config, consume_until, cx, stack, stack_with, Stack, MATE, WS_A,
    };
    use memory::types::api::{FactConflictsRequest, ImportRequest, ImportResult};

    fn triple(subject: &str, predicate: &str, object: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"subject": subject, "predicate": predicate, "object": object})
    }

    fn thing(name: &str) -> serde_json::Value {
        serde_json::json!({"kind": "entity", "name": name})
    }

    fn body(content: serde_json::Value, extra: serde_json::Value) -> serde_json::Value {
        let mut v = serde_json::json!({"content": content, "visibility": "team-shared"});
        if let (Some(o), Some(x)) = (v.as_object_mut(), extra.as_object()) {
            for (k, val) in x {
                o.insert(k.clone(), val.clone());
            }
        }
        v
    }

    async fn remember(s: &Stack, key: &str, b: serde_json::Value) {
        s.service
            .remember(cx(WS_A, Some(key)), b.to_string().as_bytes())
            .await
            .unwrap_or_else(|e| panic!("remember failed: {}", e.error));
    }

    async fn refused(s: &Stack, key: &str, b: serde_json::Value) -> String {
        // Many refusals in a row: let the write bucket refill so none reads as RATE_LIMITED.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        let e = s
            .service
            .remember(cx(WS_A, Some(key)), b.to_string().as_bytes())
            .await
            .err()
            .expect("refused");
        assert_eq!(e.error.code(), "VAL_INVALID_BODY");
        format!("{}", e.error)
    }

    async fn all_facts(s: &Stack) -> Vec<Fact> {
        s.storage
            .list_facts_page(&a_ctx(), None, true, 100)
            .await
            .unwrap()
    }

    /// Run the write consumer until `n` facts of workspace A exist.
    async fn settle(s: &Stack, n: usize) {
        let storage = s.storage.clone();
        consume_until(s, || {
            let storage = storage.clone();
            async move {
                storage
                    .list_facts_page(&a_ctx(), None, true, 100)
                    .await
                    .unwrap()
                    .len()
                    >= n
            }
        })
        .await;
        // The job that wrote the n-th fact may still be at step 8″.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    /// The fact whose object (entity name or value) is `object`.
    async fn fact_of(s: &Stack, object: &str) -> Fact {
        let all = all_facts(s).await;
        all.into_iter()
            .find(|f| {
                f.content.pointer("/object/name").and_then(|v| v.as_str()) == Some(object)
                    || f.content.pointer("/object/value").and_then(|v| v.as_str()) == Some(object)
                    || f.content.pointer("/object/amount").and_then(|v| v.as_str()) == Some(object)
            })
            .unwrap_or_else(|| panic!("no fact with object {object}"))
    }

    async fn entity_id(s: &Stack, name: &str) -> String {
        s.storage
            .find_entity_by_name(&a_ctx(), name)
            .await
            .unwrap()
            .first()
            .unwrap_or_else(|| panic!("entity {name}"))
            .id
            .clone()
    }

    async fn conflicts(s: &Stack, id: &str) -> Vec<String> {
        s.service
            .fact_conflicts(
                cx(WS_A, None),
                FactConflictsRequest {
                    ids: vec![id.into()],
                },
            )
            .await
            .ok()
            .expect("the call succeeds")
            .data
            .items
            .remove(0)
            .with
    }

    async fn manager(s: &Stack, key: &str, name: &str, extra: serde_json::Value) {
        remember(
            s,
            key,
            body(triple("Kestrel", "accountManager", thing(name)), extra),
        )
        .await;
    }

    /// Happy path — a triple with an entity object asserts its link (write-pipeline scenario 1), and
    /// the enqueued payload carries the flags (service-layer scenario 1).
    // spec: components/write-pipeline#SA-TRIPLE-04
    pub async fn a_triple_with_an_entity_object_asserts_its_link(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        settle(&s, 1).await;
        let f = fact_of(&s, "Tom Reid").await;
        let (k, t) = (
            entity_id(&s, "Kestrel").await,
            entity_id(&s, "Tom Reid").await,
        );
        let key = f.triple_key.clone().expect("a triple key");
        assert_eq!(
            (
                key.subject_entity.as_str(),
                key.predicate.as_str(),
                key.object_entity.as_deref()
            ),
            (k.as_str(), "accountManager", Some(t.as_str()))
        );
        assert!(f.single_valued && !f.changed);
        let live = s
            .storage
            .find_live_relationship(&a_ctx(), &k, "accountManager", &t)
            .await
            .unwrap()
            .expect("one live relationship of the predicate as its kind");
        let rows = s.storage.links_of_fact(&a_ctx(), &f.id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].0.id.clone(), rows[0].1),
            (live.id, LinkRole::Asserted)
        );
    }

    /// Happy path — a value object asserts no link; its canonical form is stored.
    // spec: components/write-pipeline#SA-TRIPLE-02
    pub async fn a_value_object_asserts_no_link(h: Harness) {
        let s = stack(h);
        remember(
            &s,
            "k-1",
            body(
                triple(
                    "Order 7731",
                    "price",
                    serde_json::json!({"kind": "money", "amount": "12.50", "currency": "GBP"}),
                ),
                serde_json::json!({}),
            ),
        )
        .await;
        settle(&s, 1).await;
        let f = fact_of(&s, "12.50").await;
        let key = f.triple_key.clone().unwrap();
        assert_eq!(
            (
                key.object_kind,
                key.object_value.as_str(),
                key.object_unit.as_deref()
            ),
            (ObjectKind::Money, "12.5", Some("GBP"))
        );
        assert!(s
            .storage
            .links_of_fact(&a_ctx(), &f.id)
            .await
            .unwrap()
            .is_empty());
    }

    /// Edge — a stated change ends the earlier value; no conflict is written.
    // spec: components/write-pipeline#SA-TRIPLE-06
    pub async fn a_stated_change_ends_the_earlier_value(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true, "changed": true}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert_eq!(f1.valid_to, Some(f2.valid_from));
        assert_eq!(f1.superseded_by.as_deref(), Some(f2.id.as_str()));
        assert_eq!(f2.supersedes.as_deref(), Some(f1.id.as_str()));
        assert!(f2.changed && f2.valid_to.is_none());
        let (k, t) = (
            entity_id(&s, "Kestrel").await,
            entity_id(&s, "Tom Reid").await,
        );
        assert!(s
            .storage
            .find_live_relationship(&a_ctx(), &k, "accountManager", &t)
            .await
            .unwrap()
            .is_none());
        assert!(conflicts(&s, &f2.id).await.is_empty());
    }

    /// Edge — a disagreement is recorded, not resolved; a replay records nothing twice.
    // spec: components/write-pipeline#SA-TRIPLE-08
    pub async fn a_disagreement_is_recorded_not_resolved(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert!(f1.valid_to.is_none() && f2.valid_to.is_none());
        assert_eq!(conflicts(&s, &f1.id).await, vec![f2.id.clone()]);
        // Edge — replay ends and records nothing twice.
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(conflicts(&s, &f1.id).await, vec![f2.id.clone()]);
        assert_eq!(all_facts(&s).await.len(), 2);
    }

    /// Edge — a conflict reaches a fact of another visibility and collection of the same owner.
    pub async fn a_conflict_reaches_another_visibility_and_collection(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true, "visibility": "tenant-shared"}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true, "collection": "team:folder-1"}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert!(f1.valid_to.is_none() && f2.valid_to.is_none());
        assert_eq!(conflicts(&s, &f2.id).await, vec![f1.id]);
    }

    /// Edge — a stated change ends only facts of its own visibility and collection.
    pub async fn a_stated_change_ends_only_its_own_place(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true, "visibility": "tenant-shared"}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true, "changed": true}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert!(f1.valid_to.is_none(), "another visibility is not ended");
        assert_eq!(conflicts(&s, &f1.id).await, vec![f2.id]);
    }

    /// Happy path — the fact and its link carry the writer's confidence (SA-TRIPLE-14).
    // spec: components/write-pipeline#SA-TRIPLE-14
    pub async fn the_fact_and_its_link_carry_the_writers_confidence(h: Harness) {
        let mut c = config();
        // 0.6 × 0.4 + 0.4 × 1.0 = 0.64: admitted under this gate (the scenario's "the gate admits it").
        c.trust_admit = 0.6;
        let s = stack_with(h, c);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"confidence": 0.4}),
        )
        .await;
        settle(&s, 1).await;
        let f = fact_of(&s, "Tom Reid").await;
        assert!((f.confidence - 0.4).abs() < 1e-9, "{}", f.confidence);
        let rows = s.storage.links_of_fact(&a_ctx(), &f.id).await.unwrap();
        assert!((rows[0].0.confidence - 0.4).abs() < 1e-9);
    }

    /// Happy path — a stated start; a change ends the earlier fact at it.
    pub async fn a_stated_start(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true, "valid_from": "2025-01-01T00:00:00.000Z"}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true, "changed": true,
                               "valid_from": "2025-06-01T00:00:00.000Z"}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert_eq!(f1.valid_from, at("2025-01-01T00:00:00.000Z"));
        assert_eq!(f2.valid_from, at("2025-06-01T00:00:00.000Z"));
        assert_eq!(f1.valid_to, Some(at("2025-06-01T00:00:00.000Z")));
    }

    /// Edge — an explicit link equal to the implied one keeps the predicate as its kind and its own
    /// confidence; no lower-cased kind is written.
    pub async fn an_explicit_link_equal_to_the_implied_one_keeps_the_predicate(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"links": [{"from": "Kestrel", "kind": "accountManager",
                                          "to": "Tom Reid", "confidence": 0.7}]}),
        )
        .await;
        settle(&s, 1).await;
        let f = fact_of(&s, "Tom Reid").await;
        let rows = s.storage.links_of_fact(&a_ctx(), &f.id).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.kind, "accountManager");
        assert!((rows[0].0.confidence - 0.7).abs() < 1e-9);
        let kinds = s.storage.relationship_kinds(&a_ctx(), 10).await.unwrap();
        assert!(!kinds.contains(&"accountmanager".to_string()), "{kinds:?}");
    }

    /// Edge — two values of a multi-valued predicate do not conflict.
    pub async fn two_values_of_a_multi_valued_predicate_do_not_conflict(h: Harness) {
        let s = stack(h);
        remember(
            &s,
            "k-1",
            body(
                triple("Borealis", "supplies", thing("packaging")),
                serde_json::json!({}),
            ),
        )
        .await;
        settle(&s, 1).await;
        remember(
            &s,
            "k-2",
            body(
                triple("Borealis", "supplies", thing("cartons")),
                serde_json::json!({}),
            ),
        )
        .await;
        settle(&s, 2).await;
        let (a, b) = (fact_of(&s, "packaging").await, fact_of(&s, "cartons").await);
        assert!(a.valid_to.is_none() && b.valid_to.is_none());
        assert!(conflicts(&s, &a.id).await.is_empty());
    }

    /// Edge — a triple that ended (SA-TRIPLE-07).
    // spec: components/write-pipeline#SA-TRIPLE-07
    pub async fn a_triple_that_ended(h: Harness) {
        let s = stack(h);
        remember(
            &s,
            "k-1",
            body(
                triple("Borealis", "supplies", thing("packaging")),
                serde_json::json!({}),
            ),
        )
        .await;
        settle(&s, 1).await;
        let f1 = fact_of(&s, "packaging").await;
        remember(
            &s,
            "k-2",
            body(
                triple("Borealis", "supplies", thing("packaging")),
                serde_json::json!({"ended": true}),
            ),
        )
        .await;
        settle(&s, 2).await;
        let all = all_facts(&s).await;
        let f1 = all.iter().find(|f| f.id == f1.id).unwrap();
        let f2 = all.iter().find(|f| f.id != f1.id).unwrap();
        assert!(f1.valid_to.is_some());
        assert_eq!(f1.superseded_by.as_deref(), Some(f2.id.as_str()));
        assert_eq!(
            f2.valid_to,
            Some(f2.valid_from),
            "kept as the evidence that it stopped"
        );
        let (b, p) = (
            entity_id(&s, "Borealis").await,
            entity_id(&s, "packaging").await,
        );
        assert!(s
            .storage
            .find_live_relationship(&a_ctx(), &b, "supplies", &p)
            .await
            .unwrap()
            .is_none());
    }

    /// Edge — an agent ends only facts it wrote (SA-TRIPLE-15, write-pipeline step 8″.6).
    // spec: phase-1-analysis#SA-TRIPLE-15
    pub async fn an_agent_ends_only_facts_it_wrote(h: Harness) {
        let s = stack(h);
        let supplies = || triple("Borealis", "supplies", thing("packaging"));
        remember(
            &s,
            "k-1",
            body(supplies(), serde_json::json!({"author": "agent:A"})),
        )
        .await;
        settle(&s, 1).await;
        let f1 = fact_of(&s, "packaging").await;
        let (b, p) = (
            entity_id(&s, "Borealis").await,
            entity_id(&s, "packaging").await,
        );
        let link_live = |s: &Stack| {
            let (storage, b, p) = (s.storage.clone(), b.clone(), p.clone());
            async move {
                storage
                    .find_live_relationship(&a_ctx(), &b, "supplies", &p)
                    .await
                    .unwrap()
                    .is_some()
            }
        };
        remember(
            &s,
            "k-2",
            body(
                supplies(),
                serde_json::json!({"author": "agent:B", "ended": true}),
            ),
        )
        .await;
        settle(&s, 2).await;
        let all = all_facts(&s).await;
        let first = all.iter().find(|f| f.id == f1.id).unwrap();
        assert!(first.valid_to.is_none(), "another agent ends nothing");
        assert!(link_live(&s).await, "and its link stays live");
        let second = all.iter().find(|f| f.id != f1.id).unwrap();
        assert_eq!(
            second.valid_to,
            Some(second.valid_from),
            "the new fact is stored ended"
        );
        remember(
            &s,
            "k-3",
            body(
                supplies(),
                serde_json::json!({"author": "agent:A", "ended": true}),
            ),
        )
        .await;
        settle(&s, 3).await;
        let first = s.storage.get_fact(&a_ctx(), &f1.id).await.unwrap().unwrap();
        assert!(first.valid_to.is_some(), "its author ends it");
        assert!(!link_live(&s).await, "and its link closes");
    }

    /// Edge — a write with no author ends an agent's fact; an agent's change of another agent's
    /// fact is recorded as a conflict instead.
    pub async fn a_write_with_no_author_ends_an_agents_fact(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true, "author": "agent:A"}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true, "changed": true, "author": "agent:B"}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert!(f1.valid_to.is_none(), "another agent's change ends nothing");
        assert_eq!(conflicts(&s, &f1.id).await, vec![f2.id.clone()]);
        remember(
            &s,
            "k-3",
            body(
                triple("Kestrel", "accountManager", thing("Lee Park")),
                serde_json::json!({"single_valued": true, "changed": true}),
            ),
        )
        .await;
        settle(&s, 3).await;
        let f3 = fact_of(&s, "Lee Park").await;
        let all = all_facts(&s).await;
        for f in all.iter().filter(|f| f.id != f3.id) {
            assert!(
                f.valid_to.is_some(),
                "{} is ended by a write with no author",
                f.id
            );
        }
    }

    /// Edge — a passage keeps its free content: no triple fields and no conflict check.
    // spec: components/write-pipeline#SA-TRIPLE-11
    pub async fn a_passage_keeps_its_free_content(h: Harness) {
        let s = stack(h);
        let content = serde_json::json!({"statement": "Returns are accepted within 30 days.",
                                         "subject": "Returns policy"});
        remember(
            &s,
            "k-1",
            body(
                content.clone(),
                serde_json::json!({"fact_class": "passage"}),
            ),
        )
        .await;
        settle(&s, 1).await;
        let f = all_facts(&s).await.remove(0);
        assert_eq!(f.content, content);
        assert!(f.triple_key.is_none() && !f.single_valued);
        assert_eq!(f.fact_class.as_deref(), Some("passage"));
    }

    /// Service-layer Scenario Outline — content that is not one triple, and the flag, confidence and
    /// start rows; nothing is enqueued.
    // spec: components/service-layer#SA-TRIPLE-01
    pub async fn remember_refuses_what_is_not_one_triple(h: Harness) {
        let s = stack(h);
        let cases = [
            (
                serde_json::json!({"statement": "Borealis supplies packaging"}),
                "content",
            ),
            (
                serde_json::json!({"subject": "", "predicate": "owns", "object": {"kind": "text", "value": "x"}}),
                "content.subject",
            ),
            (
                triple(
                    "Order 7731",
                    "price",
                    serde_json::json!({"kind": "money", "amount": "12.5", "currency": "gbp"}),
                ),
                "content.object",
            ),
            (
                triple(
                    "Kestrel",
                    "foundingDate",
                    serde_json::json!({"kind": "date", "value": "1998-02-30"}),
                ),
                "content.object",
            ),
        ];
        for (i, (content, field)) in cases.into_iter().enumerate() {
            let m = refused(
                &s,
                &format!("bad-{i}"),
                body(content, serde_json::json!({})),
            )
            .await;
            assert!(m.ends_with(field), "{m} names {field}");
        }
        let ok = triple("Kestrel", "accountManager", thing("Tom Reid"));
        for (key, extra, field) in [
            ("f-1", serde_json::json!({"changed": true}), "changed"),
            (
                "f-2",
                serde_json::json!({"single_valued": true, "changed": true, "ended": true}),
                "ended",
            ),
            ("f-3", serde_json::json!({"confidence": 1.5}), "confidence"),
            ("f-4", serde_json::json!({"confidence": -0.1}), "confidence"),
            (
                "f-5",
                serde_json::json!({"valid_from": (Utc::now() + chrono::Duration::days(1)).to_rfc3339()}),
                "valid_from",
            ),
        ] {
            let m = refused(&s, key, body(ok.clone(), extra)).await;
            assert!(m.ends_with(field), "{m} names {field}");
        }
        for flag in ["single_valued", "changed", "ended"] {
            let m = refused(
                &s,
                flag,
                body(
                    serde_json::json!({"statement": "a section"}),
                    serde_json::json!({"fact_class": "passage", flag: true}),
                ),
            )
            .await;
            assert!(m.ends_with(flag), "{m}");
        }
        // Nothing was enqueued: the consumer finds no work.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(all_facts(&s).await.is_empty());
    }

    /// Happy path — reading conflicts; Edge — one with a fact the reader cannot read is not shown;
    /// Error — 0, 101 or malformed ids.
    // spec: components/service-layer#SA-TRIPLE-13
    pub async fn fact_conflicts_reads_what_the_caller_may_read(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        settle(&s, 1).await;
        manager(
            &s,
            "k-2",
            "Ana Silva",
            serde_json::json!({"single_valued": true, "visibility": "user-private"}),
        )
        .await;
        settle(&s, 2).await;
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        let items = s
            .service
            .fact_conflicts(
                cx(WS_A, None),
                FactConflictsRequest {
                    ids: vec![
                        f1.id.clone(),
                        "fact:0192e7a0-0000-7000-8000-0000000000ff".into(),
                    ],
                },
            )
            .await
            .ok()
            .expect("the call succeeds")
            .data
            .items;
        assert_eq!(items[0].with, vec![f2.id.clone()]);
        assert!(
            items[1].with.is_empty(),
            "an id with no row is returned with with: []"
        );
        let mate = s
            .service
            .fact_conflicts(
                cx(MATE, None),
                FactConflictsRequest {
                    ids: vec![f1.id.clone()],
                },
            )
            .await
            .ok()
            .expect("the call succeeds")
            .data
            .items;
        assert!(mate[0].with.is_empty(), "{mate:?}");
        let many: Vec<String> = (0..101)
            .map(|i| format!("fact:0192e7a0-0000-7000-8000-{i:012}"))
            .collect();
        for bad in [vec![], many, vec!["not-a-fact".to_string()]] {
            let e = s
                .service
                .fact_conflicts(cx(WS_A, None), FactConflictsRequest { ids: bad })
                .await
                .err()
                .unwrap();
            assert_eq!(e.error.code(), "VAL_OUT_OF_RANGE");
        }
    }

    /// Retrieval — a recalled fact names its conflict; the order is unchanged by it.
    // spec: components/retrieval-engine#SA-TRIPLE-13
    pub async fn a_recalled_fact_names_its_conflict(h: Harness) {
        let s = stack(h);
        manager(&s, "k-1", "Tom Reid", serde_json::json!({})).await;
        manager(&s, "k-2", "Ana Silva", serde_json::json!({})).await;
        settle(&s, 2).await;
        let recall = |s: &Stack| {
            let svc = s.service.clone();
            async move {
                svc.recall(
                    cx(WS_A, None),
                    br#"{"query":"Kestrel account manager","result_cap":10}"#,
                )
                .await
                .ok()
                .expect("the call succeeds")
                .data
                .response
                .facts
            }
        };
        let before: Vec<String> = recall(&s).await.iter().map(|f| f.fact.id.clone()).collect();
        let (f1, f2) = (
            fact_of(&s, "Tom Reid").await,
            fact_of(&s, "Ana Silva").await,
        );
        assert!(s
            .storage
            .record_conflict(&a_ctx(), &f1.id, &f2.id, Utc::now())
            .await
            .unwrap());
        let after = recall(&s).await;
        assert_eq!(
            after.iter().map(|f| f.fact.id.clone()).collect::<Vec<_>>(),
            before,
            "a conflict neither lowers nor raises a fact"
        );
        for r in &after {
            let other = if r.fact.id == f1.id { &f2.id } else { &f1.id };
            assert_eq!(&r.conflicts, &vec![other.clone()]);
            let json = serde_json::to_value(r).unwrap();
            assert_eq!(json["conflicts"], serde_json::json!([other]));
        }
        // With no conflict, the key is absent.
        s.storage
            .end_validity(&a_ctx(), &f2.id, Utc::now())
            .await
            .unwrap();
        let alone = recall(&s).await;
        let json = serde_json::to_value(&alone[0]).unwrap();
        assert!(json.get("conflicts").is_none(), "{json}");
    }

    /// Retrieval Error row — the side-read fails: the answer is returned with every `conflicts` empty.
    /// Run on the SQL storage, where the table can be taken away under the store; a SurrealDB run
    /// returns at once.
    pub async fn the_conflict_side_read_fails_open(h: Harness) {
        #[cfg(not(feature = "sql"))]
        let _ = h;
        #[cfg(feature = "sql")]
        {
            let pool = h.pool.clone();
            let s = stack(h);
            let Some(pool) = pool else {
                return;
            };
            manager(&s, "k-1", "Tom Reid", serde_json::json!({})).await;
            settle(&s, 1).await;
            let drop = "DROP TABLE mem_fact_conflict";
            match &pool {
                memory::store::sql::SqlPool::Sqlite(p) => {
                    sqlx::query(drop).execute(p).await.unwrap();
                }
                memory::store::sql::SqlPool::Postgres(p) => {
                    sqlx::query(drop).execute(p).await.unwrap();
                }
            }
            let got = s
                .service
                .recall(cx(WS_A, None), br#"{"query":"Kestrel","result_cap":10}"#)
                .await
                .ok()
                .expect("the call succeeds")
                .data
                .response
                .facts;
            assert!(!got.is_empty());
            assert!(got.iter().all(|f| f.conflicts.is_empty()));
        }
    }

    /// Happy path — an imported fact that disagrees records a conflict (SA-TRIPLE-12); Edge — an
    /// older fact is imported unchanged; an imported triple naming no entity is refused.
    // spec: components/service-layer#SA-TRIPLE-12
    pub async fn an_imported_fact_that_disagrees_records_a_conflict(h: Harness) {
        let s = stack(h);
        manager(
            &s,
            "k-1",
            "Tom Reid",
            serde_json::json!({"single_valued": true}),
        )
        .await;
        settle(&s, 1).await;
        let f1 = fact_of(&s, "Tom Reid").await;
        let k = entity_id(&s, "Kestrel").await;
        let ana = Entity {
            private_aliases: vec![],
            alias_folders: vec![],
            id: "entity:0192e7a0-0000-7000-8000-0000000000a1".into(),
            canonical_name: "Ana Silva".into(),
            aliases: vec![],
            owner: a_owner(),
            collection: "team:folder-1".into(),
        };
        let mut f2 = super::fact(
            "fact:0192e7a0-0000-7000-8000-0000000000f2",
            "x",
            &[],
            manager_content("Kestrel", "Ana Silva"),
        );
        f2.owner = a_owner();
        f2.entities = vec![k.clone(), ana.id.clone()];
        f2.collection = "team:folder-1".into();
        f2.single_valued = true;
        f2.valid_from = Utc::now();
        f2.ingested_at = f2.valid_from;
        let mut older = super::fact(
            "fact:0192e7a0-0000-7000-8000-0000000000f3",
            "x",
            &[],
            serde_json::json!({"statement": "Borealis supplies packaging", "subject": "Borealis"}),
        );
        older.owner = a_owner();
        older.entities = vec![k.clone()];
        let mut nameless = super::fact(
            "fact:0192e7a0-0000-7000-8000-0000000000f4",
            "x",
            &[],
            manager_content("Nobody Known", "Ana Silva"),
        );
        nameless.owner = a_owner();
        nameless.entities = vec![k.clone()];
        let results = s
            .service
            .import_scored_facts(
                cx(WS_A, None),
                ImportRequest {
                    facts: vec![f2.clone(), older.clone(), nameless.clone()],
                    entities: vec![ana],
                    sources: vec![],
                    relationships: vec![],
                },
            )
            .await
            .ok()
            .expect("the call succeeds")
            .data
            .results;
        let outcome = |id: &str| results.iter().find(|(i, _)| i == id).unwrap().1.clone();
        assert_eq!(outcome(&f2.id), ImportResult::Admitted, "{results:?}");
        assert_eq!(outcome(&older.id), ImportResult::Admitted, "{results:?}");
        assert!(
            matches!(outcome(&nameless.id), ImportResult::Invalid { ref reason } if reason == "fact entities"),
            "{results:?}"
        );
        let f1_now = s.storage.get_fact(&a_ctx(), &f1.id).await.unwrap().unwrap();
        assert!(f1_now.valid_to.is_none(), "an import ends no fact");
        assert_eq!(conflicts(&s, &f2.id).await, vec![f1.id.clone()]);
        let stored_older = s
            .storage
            .get_fact(&a_ctx(), &older.id)
            .await
            .unwrap()
            .unwrap();
        assert!(stored_older.triple_key.is_none());
        assert_eq!(stored_older.content, older.content);
    }

    /// Write-pipeline Error row — a stated start later than the job's creation (defence): Rejected.
    pub async fn a_start_later_than_the_job_is_rejected(h: Harness) {
        let s = stack(h);
        let created = Utc::now();
        let job = memory::types::job::WorkJob {
            id: "work_job:0192e7a0-0000-7000-8000-00000000000a".into(),
            kind: memory::types::job::JobKind::ExtractFact,
            payload: serde_json::json!({
                "content": triple("Kestrel", "accountManager", thing("Tom Reid")),
                "valid_from": created + chrono::Duration::days(1),
            }),
            scope: a_owner(),
            idempotency_key: Some("k-future".into()),
            attempts: 0,
            status: memory::types::job::JobStatus::Pending,
            not_before: created,
            created_at: created,
            leased_until: None,
        };
        s.storage.enqueue(job).await.expect("enqueued");
        let job = s
            .storage
            .claim(
                "ordin",
                &[memory::types::job::JobKind::ExtractFact],
                std::time::Duration::from_secs(30),
            )
            .await
            .ok()
            .flatten()
            .expect("the job is claimed");
        let outcome = s.pipeline.process(&a_ctx(), &job).await.expect("processed");
        assert_eq!(outcome, memory::write_pipeline::WriteOutcome::Rejected);
        assert!(all_facts(&s).await.is_empty());
    }
}

/// The triple tests, one `#[tokio::test]` per function, for one backend.
#[macro_export]
macro_rules! triple_conformance {
    ($factory:path) => {
        macro_rules! case {
            ($name:ident) => {
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $name() {
                    $crate::triple_cases::$name($factory().await).await;
                }
            };
        }
        macro_rules! svc {
            ($name:ident) => {
                #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
                async fn $name() {
                    $crate::triple_cases::service::$name($factory().await).await;
                }
            };
        }
        case!(a_triples_parts_are_stored_and_found_by_one_lookup);
        case!(a_conflict_is_recorded_and_read);
        case!(the_lookup_reaches_every_visibility_and_collection);
        case!(a_conflict_across_owners_is_refused);
        case!(a_conflict_goes_when_one_side_ends);
        case!(a_conflict_with_an_ended_fact_is_not_written);
        case!(the_last_fact_takes_its_entity_and_a_persons_link);
        case!(an_entity_another_fact_names_stays);
        case!(earlier_facts_keep_their_shape);
        case!(a_passage_stores_no_triple);
        case!(the_triple_store_refuses_what_its_error_table_names);
        case!(a_conflict_hides_a_fact_the_reader_cannot_read);
        case!(erasures_take_the_conflict_rows);
        case!(a_triple_follows_an_entity_merge);
        svc!(a_triple_with_an_entity_object_asserts_its_link);
        svc!(a_value_object_asserts_no_link);
        svc!(a_stated_change_ends_the_earlier_value);
        svc!(a_disagreement_is_recorded_not_resolved);
        svc!(a_conflict_reaches_another_visibility_and_collection);
        svc!(a_stated_change_ends_only_its_own_place);
        svc!(the_fact_and_its_link_carry_the_writers_confidence);
        svc!(a_stated_start);
        svc!(an_explicit_link_equal_to_the_implied_one_keeps_the_predicate);
        svc!(two_values_of_a_multi_valued_predicate_do_not_conflict);
        svc!(a_triple_that_ended);
        svc!(a_passage_keeps_its_free_content);
        svc!(an_agent_ends_only_facts_it_wrote);
        svc!(a_write_with_no_author_ends_an_agents_fact);
        svc!(remember_refuses_what_is_not_one_triple);
        svc!(fact_conflicts_reads_what_the_caller_may_read);
        svc!(a_recalled_fact_names_its_conflict);
        svc!(the_conflict_side_read_fails_open);
        svc!(an_imported_fact_that_disagrees_records_a_conflict);
        svc!(a_start_later_than_the_job_is_rejected);
    };
}
