//! Row <-> domain conversion helpers for the SurrealDB 3.x store.
//!
//! SurrealDB 3.x dropped the serde-based bind/take ergonomics: `bind` takes a value implementing
//! `SurrealValue` and `take` returns a `SurrealValue`. `serde_json::Value` implements `SurrealValue`,
//! so it is the bridge in both directions:
//!
//! * **Write** — domain records are turned into a native `surrealdb::types::Object` so datetime,
//!   numeric, and embedding fields are stored as their native SurrealDB kinds (native `datetime` is
//!   required for the bi-temporal `valid_to IS NONE` / `valid_from <= valid_at` comparisons to order
//!   correctly). Caller content is stored verbatim as an object.
//! * **Read** — a row is taken as `serde_json::Value` (SurrealDB renders `datetime` -> RFC3339 string,
//!   `record id` -> "table:key" string), then deserialised into the domain struct via serde; chrono's
//!   `DateTime<Utc>` parses the RFC3339 strings natively.

use serde_json::Value as Json;
use surrealdb::types::{Datetime, Number, Object, Value};

use crate::types::domain::{Entity, Fact, Relationship, Source};
use crate::types::ports::{AuditEntry, StoreError};
use crate::write_pipeline::QuarantineRecord;

/// Convert a chrono UTC timestamp into a native SurrealDB datetime value.
pub(super) fn dt(at: chrono::DateTime<chrono::Utc>) -> Value {
    Value::Datetime(Datetime::from(at))
}

/// Convert an `f64` into a native SurrealDB float value.
fn f(v: f64) -> Value {
    Value::Number(Number::Float(v))
}

/// Convert an owned string into a native SurrealDB string value.
fn s(v: String) -> Value {
    Value::String(v)
}

/// Convert an optional string into a native SurrealDB value (`NONE` when absent).
fn opt_s(v: Option<String>) -> Value {
    match v {
        Some(x) => Value::String(x),
        None => Value::None,
    }
}

/// Convert a list of strings into a native SurrealDB array value.
fn arr_s(v: &[String]) -> Value {
    Value::Array(surrealdb::types::Array::from(v.to_vec()))
}

/// Convert the owning scope into a native SurrealDB object value.
fn owner_obj(owner: &crate::types::scope::ScopeRef) -> Value {
    let mut o = Object::new();
    o.insert("tenant", Value::String(owner.tenant.clone()));
    o.insert(
        "team",
        match &owner.team {
            Some(t) => Value::String(t.clone()),
            None => Value::None,
        },
    );
    o.insert("user", Value::String(owner.user.clone()));
    Value::Object(o)
}

/// Convert a caller-supplied `serde_json::Value` (an assertion object) into a native SurrealDB value.
fn json_to_value(j: &Json) -> Value {
    use surrealdb::types::SurrealValue;
    j.clone().into_value()
}

/// Shared with the write pipeline, which embeds the same flattened text (`crate::store`).
pub use crate::store::content_to_text;

/// Build the native row object for a `Fact` (without the `id`, which is the record key). The optional
/// `embedding`/`embedding_model` columns are written only when present on the persisted struct path;
/// `put_fact` writes them as `NONE` and C4/C7 fill them via `set_fact_embedding`.
pub fn fact_to_object(fc: &Fact) -> Object {
    let mut o = Object::new();
    o.insert("content", json_to_value(&fc.content));
    o.insert("content_text", s(content_to_text(&fc.content)));
    o.insert("entities", arr_s(&fc.entities));
    o.insert("source_id", opt_s(fc.source_id.clone()));
    o.insert(
        "memory_class",
        s(memory_class_str(fc.memory_class).to_string()),
    );
    // `fact` is SCHEMALESS, so this needs no DDL — unlike `audit_log` (0005) and `quarantine` (0006),
    // which are SCHEMAFULL and reject an undeclared key outright.
    // spec: phase-1-analysis#SA-AUTHZ-09
    o.insert("fact_class", fc.fact_class.clone().map_or(Value::None, s));
    o.insert("visibility", s(visibility_str(fc.visibility).to_string()));
    o.insert("owner", owner_obj(&fc.owner));
    o.insert("valid_from", dt(fc.valid_from));
    o.insert(
        "valid_to",
        match fc.valid_to {
            Some(t) => dt(t),
            None => Value::None,
        },
    );
    o.insert("ingested_at", dt(fc.ingested_at));
    o.insert("confidence", f(fc.confidence));
    o.insert("salience", f(fc.salience));
    o.insert("stability", f(fc.stability));
    o.insert("pii_review", Value::Bool(fc.pii_review));
    o.insert("supersedes", opt_s(fc.supersedes.clone()));
    o.insert("superseded_by", opt_s(fc.superseded_by.clone()));
    o.insert("derived_from", arr_s(&fc.derived_from));
    o.insert(
        "last_recalled_at",
        match fc.last_recalled_at {
            Some(t) => dt(t),
            None => Value::None,
        },
    );
    // Always written, "" included: the field is declared with a default by migration 0007, and a
    // written key keeps the row equal to what a later read decodes (SA-COLL-01).
    o.insert("collection", s(fc.collection.clone()));
    o
}

/// `Entity::alias_folders` as a native SurrealDB value.
pub fn alias_folders(e: &Entity) -> Value {
    let rows = e.alias_folders.iter().map(|f| {
        let mut o = Object::new();
        o.insert("alias", s(f.alias.clone()));
        o.insert("team", opt_s(f.team.clone()));
        o.insert("collection", s(f.collection.clone()));
        Value::Object(o)
    });
    Value::Array(surrealdb::types::Array::from(rows.collect::<Vec<_>>()))
}

/// Build the native row object for an `Entity`.
pub fn entity_to_object(e: &Entity) -> Object {
    let mut o = Object::new();
    o.insert("canonical_name", s(e.canonical_name.clone()));
    o.insert("aliases", arr_s(&e.public_aliases()));
    o.insert("private_aliases", arr_s(&e.private_aliases));
    o.insert("alias_folders", alias_folders(e));
    o.insert("owner", owner_obj(&e.owner));
    o.insert("collection", s(e.collection.clone()));
    o
}

/// Build the native row object for a `Relationship`. `from`/`to` are persisted as `from_ent`/`to_ent`
/// plain string entity ids (the spec's `from`/`to` collide with SurrealQL reserved words).
pub fn relationship_to_object(r: &Relationship) -> Object {
    let mut o = Object::new();
    o.insert("kind", s(r.kind.clone()));
    o.insert("from_ent", s(r.from.clone()));
    o.insert("to_ent", s(r.to.clone()));
    o.insert("valid_from", dt(r.valid_from));
    o.insert(
        "valid_to",
        match r.valid_to {
            Some(t) => dt(t),
            None => Value::None,
        },
    );
    o.insert("ingested_at", dt(r.ingested_at));
    o.insert("confidence", f(r.confidence));
    o.insert("source_id", opt_s(r.source_id.clone()));
    o.insert("owner", owner_obj(&r.owner));
    o.insert("collection", s(r.collection.clone()));
    // Declared `option<string>` by migration 0008; `NONE` when absent.
    o.insert("replaces", opt_s(r.replaces.clone()));
    o.insert("owner_only", Value::Bool(r.owner_only));
    // Declared `option<string>` by migration 0010.
    o.insert("author", opt_s(r.author.clone()));
    o
}

/// Build the native row object for a `Source`.
pub fn source_to_object(src: &Source) -> Object {
    let mut o = Object::new();
    o.insert("origin_ref", s(src.origin_ref.clone()));
    o.insert(
        "modification_marker",
        opt_s(src.modification_marker.clone()),
    );
    o.insert("trust_signal", f(src.trust_signal));
    o.insert("owner", owner_obj(&src.owner));
    o.insert("collection", s(src.collection.clone()));
    o
}

/// Build the native row object for an `AuditEntry`.
pub fn audit_to_object(e: &AuditEntry) -> Object {
    let mut o = Object::new();
    o.insert("subject", s(e.subject.clone()));
    o.insert("operation", s(e.operation.clone()));
    o.insert("scope", owner_obj(&e.scope));
    o.insert("outcome", s(e.outcome.clone()));
    o.insert("token_jti", s(e.token_jti.clone()));
    o.insert("correlation_id", s(e.correlation_id.clone()));
    // `client` and `fact_class` are `option<string>` on a SCHEMAFULL table (migration 0005), so a
    // `None` is written as NULL rather than omitted — an omitted key on a schemafull row and a NULL
    // read back differently, and the audit trail is queried on both.
    o.insert("client", e.client.clone().map_or(Value::None, s));
    o.insert("fact_class", e.fact_class.clone().map_or(Value::None, s));
    o.insert("at", dt(e.at));
    o
}

/// Build the native row object for a `QuarantineRecord` (without the `id`, which is the record key).
/// Moved here from the write pipeline with the quarantine sink (M-1): the field set and encodings are
/// unchanged.
pub fn quarantine_to_object(qr: &QuarantineRecord) -> Object {
    let mut o = Object::new();
    o.insert("content", json_to_value(&qr.content));
    o.insert("entities", arr_s(&qr.entities));
    o.insert("source_id", opt_s(qr.source_id.clone()));
    o.insert(
        "memory_class",
        s(memory_class_str(qr.memory_class).to_string()),
    );
    o.insert("owner", owner_obj(&qr.owner));
    o.insert("trust_score", f(qr.trust_score));
    o.insert("confidence", f(qr.confidence));
    o.insert("salience", f(qr.salience));
    o.insert("is_instruction_like", Value::Bool(qr.is_instruction_like));
    o.insert("pii_review", Value::Bool(qr.pii_review));
    o.insert("idempotency_key", opt_s(qr.idempotency_key.clone()));
    o.insert("reason", s(qr.reason.clone()));
    o.insert("quarantined_at", dt(qr.quarantined_at));
    // Declared on the SCHEMAFULL `quarantine` by migration 0007 (C4's portion).
    o.insert("collection", s(qr.collection.clone()));
    // Declared by migration 0008 (C4's portion): what the held write asserted.
    o.insert(
        "entities_asserted",
        json_to_value(&serde_json::to_value(&qr.entities_asserted).unwrap_or_default()),
    );
    o.insert(
        "links_asserted",
        json_to_value(&serde_json::to_value(&qr.links_asserted).unwrap_or_default()),
    );
    o.insert("derived_from", arr_s(&qr.derived_from));
    // Declared by migration 0010.
    o.insert("fact_id", opt_s(qr.fact_id.clone()));
    o
}

/// The canonical kebab-case string for a `MemoryClass` (matches the serde rename used on the wire).
pub fn memory_class_str(c: crate::types::domain::MemoryClass) -> &'static str {
    use crate::types::domain::MemoryClass::*;
    match c {
        Episodic => "episodic",
        Semantic => "semantic",
        Consolidated => "consolidated",
    }
}

/// The canonical kebab-case string for a `Visibility` (matches the serde rename used on the wire).
pub fn visibility_str(v: crate::types::domain::Visibility) -> &'static str {
    use crate::types::domain::Visibility::*;
    match v {
        UserPrivate => "user-private",
        TeamShared => "team-shared",
        TenantShared => "tenant-shared",
    }
}

/// Normalise a row's `id` JSON value to the canonical "table:key" string. SurrealDB renders a record
/// id as that string already, so this strips any surrounding angle brackets the SQL form may add.
pub(crate) fn normalise_id(raw: &Json) -> Option<String> {
    raw.as_str()
        .map(|s| s.replace(['⟨', '⟩', '`'], "").trim().to_string())
}

/// Deserialise a fact row (taken as JSON) into a domain `Fact`. The persistence-only `embedding`,
/// `embedding_model`, and `content_text` columns are not part of the JSON struct and are ignored.
pub fn row_to_fact(mut row: Json) -> Result<Fact, StoreError> {
    fix_id(&mut row);
    if let Some(obj) = row.as_object_mut() {
        obj.remove("embedding");
        obj.remove("embedding_model");
        obj.remove("content_text");
    }
    serde_json::from_value(row).map_err(|e| StoreError::Internal(format!("decode fact: {e}")))
}

/// Deserialise an entity row into a domain `Entity`.
pub fn row_to_entity(mut row: Json) -> Result<Entity, StoreError> {
    fix_id(&mut row);
    // A row written before the field existed holds none; `aliases` is what the owner reads whole.
    if let Some(o) = row.as_object_mut() {
        let private = o.remove("private_aliases").unwrap_or(Json::Null);
        if let Json::Array(private) = private {
            if let Some(Json::Array(aliases)) = o.get_mut("aliases") {
                aliases.extend(private.iter().cloned());
            }
            o.insert("private_aliases".into(), Json::Array(private));
        }
    }
    serde_json::from_value(row).map_err(|e| StoreError::Internal(format!("decode entity: {e}")))
}

/// Deserialise a relationship row into a domain `Relationship`, mapping `from_ent`/`to_ent` back to
/// the struct's `from`/`to` fields.
pub fn row_to_relationship(mut row: Json) -> Result<Relationship, StoreError> {
    fix_id(&mut row);
    if let Some(obj) = row.as_object_mut() {
        if obj.get("owner_only").is_some_and(Json::is_null) {
            obj.remove("owner_only");
        }
        if let Some(v) = obj.remove("from_ent") {
            obj.insert("from".to_string(), v);
        }
        if let Some(v) = obj.remove("to_ent") {
            obj.insert("to".to_string(), v);
        }
    }
    serde_json::from_value(row)
        .map_err(|e| StoreError::Internal(format!("decode relationship: {e}")))
}

/// Deserialise a source row into a domain `Source`.
/// Deserialise a `quarantine` row into its record.
pub fn row_to_quarantine(mut row: Json) -> Result<QuarantineRecord, StoreError> {
    fix_id(&mut row);
    serde_json::from_value(row).map_err(|e| StoreError::Internal(format!("decode quarantine: {e}")))
}

pub fn row_to_source(mut row: Json) -> Result<Source, StoreError> {
    fix_id(&mut row);
    serde_json::from_value(row).map_err(|e| StoreError::Internal(format!("decode source: {e}")))
}

/// Normalise the `id` field of a row in place to a clean "table:key" string.
fn fix_id(row: &mut Json) {
    if let Some(obj) = row.as_object_mut() {
        if let Some(id) = obj.get("id") {
            if let Some(norm) = normalise_id(id) {
                obj.insert("id".to_string(), Json::String(norm));
            }
        }
    }
}

#[cfg(test)]
mod fact_class_tests {
    use super::*;

    /// A fact row written before `fact_class` existed must still decode. `row_to_fact` deserialises
    /// the row straight into `Fact`, so `#[serde(default)]` on the field is load-bearing rather than
    /// cosmetic: without it every pre-existing row in every deployment would fail to read
    /// (RFC 05 acceptance criterion 4).
    #[test]
    fn a_row_written_before_the_field_existed_still_decodes() {
        let row = serde_json::json!({
            "id": "fact:018f0000-0000-7000-8000-000000000001",
            "content": {"subject": "s", "predicate": "p", "object": "o"},
            "entities": ["entity:e1"],
            "source_id": null,
            "memory_class": "semantic",
            "visibility": "user-private",
            "owner": {"tenant": "acme", "team": null, "user": "u-1"},
            "valid_from": "2026-01-01T00:00:00.000Z",
            "valid_to": null,
            "ingested_at": "2026-01-01T00:00:00.000Z",
            "confidence": 0.9,
            "salience": 0.8,
            "stability": 1.0,
            "pii_review": false,
            "supersedes": null,
            "superseded_by": null,
            "derived_from": [],
            "last_recalled_at": null
        });
        let fact = row_to_fact(row).expect("a row predating fact_class must decode");
        assert_eq!(fact.fact_class, None);
    }

    /// A row carrying the field decodes it back verbatim — memory stores the caller's string and
    /// neither normalises nor interprets it.
    #[test]
    fn a_row_carrying_the_field_round_trips_it_verbatim() {
        let row = serde_json::json!({
            "id": "fact:018f0000-0000-7000-8000-000000000002",
            "content": {"subject": "s", "predicate": "p", "object": "o"},
            "entities": ["entity:e1"],
            "source_id": null,
            "memory_class": "semantic",
            "fact_class": "Medication-Authoritative",
            "visibility": "user-private",
            "owner": {"tenant": "acme", "team": null, "user": "u-1"},
            "valid_from": "2026-01-01T00:00:00.000Z",
            "valid_to": null,
            "ingested_at": "2026-01-01T00:00:00.000Z",
            "confidence": 0.9,
            "salience": 0.8,
            "stability": 1.0,
            "pii_review": false,
            "supersedes": null,
            "superseded_by": null,
            "derived_from": [],
            "last_recalled_at": null
        });
        let fact = row_to_fact(row).expect("decode");
        assert_eq!(fact.fact_class.as_deref(), Some("Medication-Authoritative"));
    }

    /// A row without `collection` decodes in the default collection, and a fact of the default
    /// collection serialises without the key; any other collection is carried (ADR-020).
    #[test]
    fn a_row_without_a_collection_decodes_in_the_default_collection() {
        let row = serde_json::json!({
            "id": "fact:018f0000-0000-7000-8000-000000000004",
            "content": {"subject": "s"},
            "entities": ["entity:e1"],
            "source_id": null,
            "memory_class": "semantic",
            "visibility": "user-private",
            "owner": {"tenant": "acme", "team": null, "user": "u-1"},
            "valid_from": "2026-01-01T00:00:00.000Z",
            "valid_to": null,
            "ingested_at": "2026-01-01T00:00:00.000Z",
            "confidence": 0.9,
            "salience": 0.8,
            "stability": 1.0,
            "pii_review": false,
            "supersedes": null,
            "superseded_by": null,
            "derived_from": [],
            "last_recalled_at": null
        });
        let mut fact = row_to_fact(row).expect("decode");
        assert_eq!(fact.collection, "");
        assert!(serde_json::to_value(&fact)
            .unwrap()
            .get("collection")
            .is_none());
        fact.collection = "tk:b".into();
        assert_eq!(
            serde_json::to_value(&fact).unwrap()["collection"],
            serde_json::json!("tk:b")
        );
    }

    /// An unclassed fact serialises without the key, so the API response omits it rather than
    /// carrying `null` (RFC 05 acceptance criterion 3).
    #[test]
    fn an_unclassed_fact_serialises_without_the_key() {
        let row = serde_json::json!({
            "id": "fact:018f0000-0000-7000-8000-000000000003",
            "content": {"subject": "s", "predicate": "p", "object": "o"},
            "entities": ["entity:e1"],
            "source_id": null,
            "memory_class": "semantic",
            "visibility": "user-private",
            "owner": {"tenant": "acme", "team": null, "user": "u-1"},
            "valid_from": "2026-01-01T00:00:00.000Z",
            "valid_to": null,
            "ingested_at": "2026-01-01T00:00:00.000Z",
            "confidence": 0.9,
            "salience": 0.8,
            "stability": 1.0,
            "pii_review": false,
            "supersedes": null,
            "superseded_by": null,
            "derived_from": [],
            "last_recalled_at": null
        });
        let fact = row_to_fact(row).expect("decode");
        let out = serde_json::to_value(&fact).expect("serialise");
        assert!(
            out.get("fact_class").is_none(),
            "an unclassed fact must omit the key, got {out}"
        );
    }
}
