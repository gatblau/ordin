//! Domain entities (§2C.2).
//!
//! Used by: C1, C4, C6, C7 (and C8 for GET fact).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::scope::ScopeRef;

#[derive(Serialize, Deserialize, Clone)]
pub struct Fact {
    /// "fact:<uuidv7>" (SA-ID-01).
    pub id: String,
    /// Structured assertion (object), not free text.
    pub content: serde_json::Value,
    /// Entity ids this fact connects (>=1).
    pub entities: Vec<String>,
    /// "source:<uuidv7>" provenance, null for agent-stated.
    pub source_id: Option<String>,
    pub memory_class: MemoryClass,
    /// The caller-owned class this fact was **authorised as** (RFC 05). Opaque to `memory`: no
    /// vocabulary, no validation, no value treated specially. `None` = written without a class.
    ///
    /// Distinct from [`MemoryClass`], which is memory's own episodic/semantic/consolidated
    /// vocabulary. `serde(default)` is load-bearing rather than cosmetic — `row_to_fact`
    /// deserialises the stored row straight into this struct, so without it every fact written
    /// before this field would fail to decode.
    // spec: phase-1-analysis#SA-AUTHZ-09
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact_class: Option<String>,
    pub visibility: Visibility,
    /// Owning (tenant, team, user).
    pub owner: ScopeRef,
    /// RFC3339 ms (SA-TIME-01).
    pub valid_from: DateTime<Utc>,
    /// Null = currently true (open interval).
    pub valid_to: Option<DateTime<Utc>>,
    /// Server-set.
    pub ingested_at: DateTime<Utc>,
    /// [0,1] (SA-SCORE-01).
    pub confidence: f64,
    /// [0,1].
    pub salience: f64,
    /// Decay stability `s` (SA-DECAY-01), >=0.0.
    pub stability: f64,
    /// True => low-confidence PII flagged for review (SA-PII-01).
    pub pii_review: bool,
    /// Fact id this one replaces.
    pub supersedes: Option<String>,
    /// Fact id replacing this one.
    pub superseded_by: Option<String>,
    /// Source fact ids (consolidated insights only).
    pub derived_from: Vec<String>,
    pub last_recalled_at: Option<DateTime<Utc>>,
    /// Caller-chosen collection label (ADR-020, SA-COLL-01); "" = the default collection.
    /// `#[serde(default)]` so rows written before migration 0007 decode.
    // spec: components/memory-store#SA-COLL-01
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
    /// The writer stated the predicate single-valued (ADR-022 (2)).
    // spec: phase-1-analysis#SA-TRIPLE-05
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub single_valued: bool,
    /// The writer stated that the earlier value stopped being true (ADR-022 (2)).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub changed: bool,
    /// The triple's indexed parts, set by C4 after entity resolution. Store-internal: never serialised
    /// to a caller. `None` for a passage, a fact written before `0011_triples`, or an imported fact
    /// whose content is not a triple (SA-TRIPLE-11, SA-TRIPLE-12).
    // spec: components/memory-store#SA-TRIPLE-01
    // `skip` rather than `default, skip_serializing`: a stored row carries the parts in fields of their
    // own, which each store reads into this; it is never decoded from a caller's JSON.
    #[serde(skip)]
    pub triple_key: Option<TripleKey>,
}

/// A triple's indexed parts (memory-store amendment ADR-022): the subject and an entity object by
/// entity id, the predicate normalised, the object in its canonical form (SA-TRIPLE-02).
// spec: phase-1-analysis#SA-TRIPLE-02
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TripleKey {
    /// "entity:<uuid>".
    pub subject_entity: String,
    /// `memory::text::normalise_predicate`.
    pub predicate: String,
    pub object_kind: ObjectKind,
    /// Canonical form (SA-TRIPLE-02); the entity id for an entity object.
    pub object_value: String,
    /// The normalised unit, or the currency.
    pub object_unit: Option<String>,
    /// Set only for [`ObjectKind::Entity`].
    pub object_entity: Option<String>,
    /// The write's author (SA-GRAPH-14); `None` for the in-process caller's own or a person's write.
    // spec: phase-1-analysis#SA-TRIPLE-15
    pub author: Option<String>,
}

/// The kind of a triple's object (SA-TRIPLE-02).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    #[default]
    Entity,
    Text,
    Number,
    Money,
    Date,
    Boolean,
}

impl ObjectKind {
    /// The stored form (`object_kind` field / column).
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectKind::Entity => "entity",
            ObjectKind::Text => "text",
            ObjectKind::Number => "number",
            ObjectKind::Money => "money",
            ObjectKind::Date => "date",
            ObjectKind::Boolean => "boolean",
        }
    }

    /// The kind a stored `object_kind` names; `None` for any other text.
    pub fn parse(s: &str) -> Option<ObjectKind> {
        Some(match s {
            "entity" => ObjectKind::Entity,
            "text" => ObjectKind::Text,
            "number" => ObjectKind::Number,
            "money" => ObjectKind::Money,
            "date" => ObjectKind::Date,
            "boolean" => ObjectKind::Boolean,
            _ => return None,
        })
    }
}

/// One folder that stated an alias of an entity: a team's collection and the alias, normalised.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AliasFolder {
    pub alias: String,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub collection: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Entity {
    /// "entity:<uuidv7>".
    pub id: String,
    /// 1..=512 chars, non-empty.
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub owner: ScopeRef,
    /// Caller-chosen collection label (ADR-020, SA-COLL-01); "" = the default collection.
    /// `#[serde(default)]` so rows written before migration 0007 decode.
    // spec: components/memory-store#SA-COLL-01
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
    /// Those of `aliases` that came from the owner's private facts: stored apart, and read by the
    /// owner alone. Empty, with those names gone from `aliases`, for every other reader.
    // See `Entity::as_seen_by`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub private_aliases: Vec<String>,
    /// The folders that stated each alias. An alias with no row here was not stated by a write, and
    /// no folder erasure removes it. Store-internal: it is never serialised to a caller.
    #[serde(default, skip_serializing)]
    pub alias_folders: Vec<AliasFolder>,
}

impl Entity {
    /// The aliases every reader of the entity may see.
    pub fn public_aliases(&self) -> Vec<String> {
        self.aliases
            .iter()
            .filter(|a| !self.private_aliases.contains(a))
            .cloned()
            .collect()
    }

    /// This entity as a reader sees it: the owner (`owner_view`) whole, anyone else without the
    /// owner's private aliases. Every store read that hands an entity to a caller ends here.
    #[must_use]
    pub fn as_seen_by(mut self, owner_view: bool) -> Self {
        if !owner_view {
            self.aliases = self.public_aliases();
            self.private_aliases.clear();
            self.alias_folders.clear();
        }
        self
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Relationship {
    /// "relationship:<uuidv7>".
    pub id: String,
    /// Typed edge label, e.g. "owns" (1..=128 chars).
    pub kind: String,
    /// Entity id.
    pub from: String,
    /// Entity id.
    pub to: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: Option<DateTime<Utc>>,
    pub ingested_at: DateTime<Utc>,
    /// [0,1].
    pub confidence: f64,
    pub source_id: Option<String>,
    pub owner: ScopeRef,
    /// Caller-chosen collection label (ADR-020, SA-COLL-01); "" = the default collection.
    /// `#[serde(default)]` so rows written before migration 0007 decode.
    // spec: components/memory-store#SA-COLL-01
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
    /// The relationship a person's correction replaced (`Service::correct_link`, SA-GRAPH-06).
    /// `#[serde(default)]` so rows written before migration 0008 decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    /// A person's correction of a link no teammate could read: it stays the owner's, whatever its
    /// team, so correcting a private link does not publish it.
    // The row keeps its team (team and folder erasure find rows by it) and this flag narrows the read.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub owner_only: bool,
    /// Who among one owner's callers wrote the link, as the in-process host names them: every agent
    /// of a workspace shares its `owner.user`. `None`: written before migration 0010, by a person or
    /// by the host itself.
    // spec: phase-1-analysis#SA-GRAPH-14
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Source {
    /// "source:<uuidv7>".
    pub id: String,
    /// Document/system handle, opaque to recall.
    pub origin_ref: String,
    /// ETag / Last-Modified token for freshness.
    pub modification_marker: Option<String>,
    /// [0,1] prior trust of this source.
    pub trust_signal: f64,
    pub owner: ScopeRef,
    /// Caller-chosen collection label (ADR-020, SA-COLL-01); "" = the default collection.
    /// `#[serde(default)]` so rows written before migration 0007 decode.
    // spec: components/memory-store#SA-COLL-01
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collection: String,
}

/// Whether `label` is a collection label: `^[a-z0-9:_-]{0,64}$` (SA-COLL-01). `""` is the default
/// collection. The store never interprets the label; this is the one definition every layer checks it
/// against.
// spec: components/memory-store#SA-COLL-01
pub(crate) fn is_valid_collection(label: &str) -> bool {
    label.len() <= 64
        && label.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b':' | b'_' | b'-')
        })
}

/// Procedural rejected (SA-CLASS-01).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum MemoryClass {
    Episodic,
    Semantic,
    Consolidated,
}

/// (SA-VIS-01).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    UserPrivate,
    TeamShared,
    TenantShared,
}
