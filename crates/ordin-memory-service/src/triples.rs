//! Facts as triples, the vocabulary, conflicts, passages and the diagram (MemoryService.md amendment
//! January 2026, ADR-118 and ADR-120): `remember_facts` (T1), the place's vocabulary (V1 to V3,
//! MEM-TR-1 to MEM-TR-3, MEM-TR-11), `keep_conflict_side` (C1), `passages` (P1), `diagram` (G1),
//! the changed `correct_fact` (§9 step 4) and rule FV′.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use memory::types::api::{FactConflictsRequest, Triple, TripleObject};
use memory::types::domain::Fact;
use memory::types::ports::PiiDetector;
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, KnowledgePageId, MemoryFactId, MemoryThingId, RunId, WorkspaceId,
};
use serde_json::json;

use crate::db::{placeholders, q};
use crate::error::MemoryError;
use crate::model::{
    DiagramJoin, DiagramThing, DiagramValue, DiagramView, FactSource, FactSourceInput, FactState,
    FactView, MemoryCaller, MemoryPlace, MergeTermReceipt, ObjectInput, ObjectKindInput,
    ObjectView, PassagePage, PassageView, PredicateInput, RememberFactsInput, RememberFactsReceipt,
    TermOrigin, TermProposal, TermRef, TermView, ThingInput, ThingRef, TripleInput, TripleView,
    VocabularyView,
};
use crate::remember::{
    agent_of, entity_assertions, sha256_hex, RowSeed, TripleSeed, Write, PAGE_ORIGIN, PASSAGE_CLASS,
};
use crate::service::{new_correlation_id, place_of, Inner, MemoryService};

use crate::store::{self, RefRow};

/// A place's most terms, merged ones included (MEM-TR-3).
pub(crate) const TERMS_MAX: u64 = 500;
/// One call's most facts (T1 step 1).
const FACTS_MAX: usize = 16;
/// A merge's most facts (V3).
const MERGE_FACTS_MAX: u64 = 500;
/// Diagram bounds (MEM-TR-10).
const DIAGRAM_THINGS: usize = 40;
const DIAGRAM_JOINS: usize = 200;
const DIAGRAM_VALUES: usize = 40;

/// One seeded term (MEM-TR-3).
pub struct SeedTerm {
    pub key: &'static str,
    pub label: &'static str,
    pub meaning: &'static str,
    pub object: ObjectKindInput,
    pub single_valued: bool,
}

const fn seed(
    key: &'static str,
    label: &'static str,
    meaning: &'static str,
    object: ObjectKindInput,
    single_valued: bool,
) -> SeedTerm {
    SeedTerm {
        key,
        label,
        meaning,
        object,
        single_valued,
    }
}

use ObjectKindInput::{Date as D, Money as M, Number as N, Text as T, Thing as TH};

/// The vocabulary's seed: schema.org properties, apart from `prefers`, which is ordin's own.
// spec: ordin-desktop/components/MemoryService#MEM-TR-3
pub const SEED_TERMS: [SeedTerm; 26] = [
    seed(
        "worksFor",
        "works for",
        "An organisation the subject works for.",
        TH,
        false,
    ),
    seed(
        "memberOf",
        "member of",
        "A group or organisation the subject belongs to.",
        TH,
        false,
    ),
    seed("owns", "owns", "Something the subject owns.", TH, false),
    seed(
        "location",
        "located in",
        "A place where the subject is, or happens.",
        TH,
        false,
    ),
    seed(
        "author",
        "written by",
        "The person or organisation that wrote the subject.",
        TH,
        false,
    ),
    seed(
        "startDate",
        "starts on",
        "The date the subject starts or started.",
        D,
        true,
    ),
    seed(
        "endDate",
        "ends on",
        "The date the subject ends or ended.",
        D,
        true,
    ),
    seed("price", "price", "What the subject costs.", M, true),
    seed(
        "amount",
        "amount",
        "An amount of money the subject is for, such as a grant or a payment.",
        M,
        true,
    ),
    seed(
        "numberOfEmployees",
        "number of employees",
        "How many people the subject employs.",
        N,
        true,
    ),
    seed("jobTitle", "job title", "The subject's job title.", T, true),
    seed(
        "parentOrganization",
        "part of organisation",
        "The larger organisation the subject belongs to.",
        TH,
        true,
    ),
    seed(
        "subOrganization",
        "includes organisation",
        "An organisation that is part of the subject.",
        TH,
        false,
    ),
    seed(
        "isPartOf",
        "part of",
        "Something the subject is a part of.",
        TH,
        false,
    ),
    seed(
        "hasPart",
        "has part",
        "Something that is a part of the subject.",
        TH,
        false,
    ),
    seed(
        "about",
        "is about",
        "What the subject, such as a document, is about.",
        TH,
        false,
    ),
    seed(
        "foundingDate",
        "founded on",
        "The date the subject was founded.",
        D,
        true,
    ),
    seed(
        "founder",
        "founded by",
        "A person or organisation that founded the subject.",
        TH,
        false,
    ),
    seed(
        "employee",
        "employs",
        "A person the subject employs.",
        TH,
        false,
    ),
    seed(
        "member",
        "has member",
        "A person or organisation that belongs to the subject.",
        TH,
        false,
    ),
    seed("url", "web address", "The subject's web address.", T, true),
    seed(
        "legalName",
        "legal name",
        "The subject's registered name.",
        T,
        true,
    ),
    seed(
        "knowsAbout",
        "knows about",
        "A topic the subject knows about.",
        TH,
        false,
    ),
    seed(
        "manufacturer",
        "made by",
        "The organisation that makes the subject.",
        TH,
        true,
    ),
    seed("brand", "brand", "A brand the subject carries.", TH, false),
    seed(
        "prefers",
        "prefers",
        "Something the subject, usually the person, prefers.",
        T,
        false,
    ),
];

/// One stored term (`memory_terms`).
#[derive(Debug, Clone)]
pub(crate) struct Term {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) meaning: String,
    pub(crate) object: ObjectKindInput,
    pub(crate) single_valued: bool,
    pub(crate) origin: String,
    pub(crate) agent_instance_id: Option<AgentInstanceId>,
    pub(crate) merged_into: Option<String>,
    pub(crate) renamed: bool,
}

const TERM_COLUMNS: &str = "term_key, label, meaning, object_kind, single_valued, origin, \
    agent_instance_id, merged_into, renamed_at";

fn term_of(row: &crate::db::R) -> Result<Term, MemoryError> {
    Ok(Term {
        key: row.text("term_key")?,
        label: row.text("label")?,
        meaning: row.text("meaning")?,
        object: ObjectKindInput::parse(&row.text("object_kind")?)
            .ok_or_else(MemoryError::internal)?,
        single_valued: row.boolean("single_valued")?,
        origin: row.text("origin")?,
        agent_instance_id: row.opt_uuid("agent_instance_id")?.map(AgentInstanceId),
        merged_into: row.opt_text("merged_into")?,
        renamed: row.opt_ts("renamed_at")?.is_some(),
    })
}

impl Term {
    fn view(&self, fact_count: u32) -> TermView {
        let added_by = match (self.origin.as_str(), self.agent_instance_id) {
            ("seed", _) => TermOrigin::Seed,
            ("agent", Some(agent_instance_id)) => TermOrigin::Agent { agent_instance_id },
            ("hosted_agent", _) => TermOrigin::HostedAgent,
            ("team", _) => TermOrigin::Team,
            ("person", _) => TermOrigin::Person,
            _ => TermOrigin::Ordin,
        };
        TermView {
            key: self.key.clone(),
            label: self.label.clone(),
            meaning: self.meaning.clone(),
            object: self.object,
            single_valued: self.single_valued,
            seed_unrenamed: self.origin == "seed" && !self.renamed,
            added_by,
            fact_count,
        }
    }
}

/// MEM-TR-1: the key a proposed label derives, before a suffix for a key already used.
// spec: ordin-desktop/components/MemoryService#MEM-TR-1
pub(crate) fn key_of_label(label: &str) -> String {
    let words: Vec<String> = label
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let mut key = String::new();
    for (at, word) in words.iter().enumerate() {
        if at == 0 {
            key.push_str(word);
        } else {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                key.push(first.to_ascii_uppercase());
                key.extend(chars);
            }
        }
    }
    if key.is_empty() {
        let digest = sha256_hex(&memory::text::normalise_name(label));
        return format!("t{}", &digest[..10]);
    }
    if key.starts_with(|c: char| c.is_ascii_digit()) {
        key.insert(0, 't');
    }
    key.chars().take(62).collect()
}

fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// MEM-TR-2: 1–64 characters, no control character.
// spec: ordin-desktop/components/MemoryService#MEM-TR-2
pub(crate) fn label_ok(label: &str) -> bool {
    (1..=64).contains(&label.trim().chars().count()) && !has_control(label)
}

/// MEM-TR-13: the longest text value a remembered fact may hold.
const FACT_TEXT_MAX: usize = 120;

/// MEM-TR-13: words that name a kind of note rather than how a thing relates to its value.
const NOTE_WORDS: [&str; 9] = [
    "key fact",
    "known issue",
    "note",
    "issue",
    "fact",
    "summary",
    "detail",
    "point",
    "info",
];

/// MEM-TR-13: the proposed label, normalised with `_` and `-` read as spaces, is a kind of note, in the
/// singular or the plural.
// spec: ordin-desktop/components/MemoryService#MEM-TR-13
pub(crate) fn is_note_word(label: &str) -> bool {
    let spaced: String = label
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    let norm = memory::text::normalise_name(&spaced);
    let singular = |w: &str| {
        norm == w
            || norm.strip_suffix('s') == Some(w)
            || (w.ends_with('y') && norm.strip_suffix("ies") == w.strip_suffix('y'))
    };
    NOTE_WORDS.iter().any(|w| singular(w))
}

/// MEM-TR-13: a text value holds one claim: at most 120 characters after trimming, and no two claims
/// joined by `;`.
// spec: ordin-desktop/components/MemoryService#MEM-TR-13
pub(crate) fn is_one_claim(value: &str) -> bool {
    value.trim().chars().count() <= FACT_TEXT_MAX
        && value
            .split(';')
            .filter(|part| !part.trim().is_empty())
            .count()
            <= 1
}

fn meaning_ok(meaning: &str) -> bool {
    (1..=300).contains(&meaning.trim().chars().count()) && !has_control(meaning)
}

fn thing_ok(thing: &ThingInput) -> bool {
    let name_ok =
        |n: &str| (1..=200).contains(&n.trim().chars().count()) && !n.chars().any(char::is_control);
    name_ok(&thing.name)
        && thing.other_names.len() <= 8
        && thing.other_names.iter().all(|n| name_ok(n))
}

/// T1 step 2.3: the component's object for an `ObjectInput`, checked by its kind (memory
/// SA-TRIPLE-02); `None` when the value does not read as its kind.
// spec: ordin-desktop/components/MemoryService#M-10
pub(crate) fn triple_object(object: &ObjectInput) -> Option<TripleObject> {
    let out = match object {
        ObjectInput::Thing(thing) => {
            if !thing_ok(thing) {
                return None;
            }
            TripleObject::Entity {
                name: thing.name.trim().to_owned(),
            }
        }
        ObjectInput::Text(value) => TripleObject::Text {
            value: value.clone(),
        },
        ObjectInput::Number { value, unit } => TripleObject::Number {
            value: value.clone(),
            unit: unit.clone(),
        },
        ObjectInput::Money { amount, currency } => TripleObject::Money {
            amount: amount.clone(),
            currency: currency.clone(),
        },
        ObjectInput::Date(value) => TripleObject::Date {
            value: value.clone(),
        },
        ObjectInput::YesNo(value) => TripleObject::Boolean { value: *value },
    };
    memory::triple::object_is_valid(&out).then_some(out)
}

/// MEM-TR-9: `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, a real date not later than `today`, as 00:00:00.000
/// UTC on its first day.
// spec: ordin-desktop/components/MemoryService#MEM-TR-9
pub(crate) fn start_of(value: &str, today: NaiveDate) -> Option<chrono::DateTime<Utc>> {
    let parts: Vec<&str> = value.split('-').collect();
    let num = |p: &str, len: usize| -> Option<u32> {
        (p.len() == len && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse().ok())
            .flatten()
    };
    let (first, last_unit) = match parts.as_slice() {
        [y] => (NaiveDate::from_ymd_opt(num(y, 4)? as i32, 1, 1)?, 0),
        [y, m] => (
            NaiveDate::from_ymd_opt(num(y, 4)? as i32, num(m, 2)?, 1)?,
            1,
        ),
        [y, m, d] => (
            NaiveDate::from_ymd_opt(num(y, 4)? as i32, num(m, 2)?, num(d, 2)?)?,
            2,
        ),
        _ => return None,
    };
    let later = match last_unit {
        0 => first.year() > today.year(),
        1 => (first.year(), first.month()) > (today.year(), today.month()),
        _ => first > today,
    };
    if later {
        return None;
    }
    Utc.from_utc_datetime(&first.and_hms_opt(0, 0, 0)?).into()
}

/// The text of an object view for `statement` (rule FV′).
fn object_text(object: &ObjectView) -> String {
    match object {
        ObjectView::Thing { thing } => thing.name.clone(),
        ObjectView::Text { value } | ObjectView::Date { value } => value.clone(),
        ObjectView::Number { value, unit } => match unit {
            Some(unit) => format!("{value} {unit}"),
            None => value.clone(),
        },
        ObjectView::Money { amount, currency } => format!("{amount} {currency}"),
        ObjectView::YesNo { value } => if *value { "yes" } else { "no" }.to_owned(),
    }
}

/// A component object kind against a term's (`thing` ↔ `entity`, `yes_no` ↔ `boolean`).
pub(crate) fn kind_matches(term: ObjectKindInput, object: &TripleObject) -> bool {
    matches!(
        (term, object),
        (ObjectKindInput::Thing, TripleObject::Entity { .. })
            | (ObjectKindInput::Text, TripleObject::Text { .. })
            | (ObjectKindInput::Number, TripleObject::Number { .. })
            | (ObjectKindInput::Money, TripleObject::Money { .. })
            | (ObjectKindInput::Date, TripleObject::Date { .. })
            | (ObjectKindInput::YesNo, TripleObject::Boolean { .. })
    )
}

/// One fact after T1 step 2's checks.
struct Checked {
    subject: ThingInput,
    topic: Option<String>,
    object: TripleObject,
    thing_object: Option<ThingInput>,
    term: Term,
    changed: bool,
    ended: bool,
    confidence: Option<f64>,
    valid_from: Option<chrono::DateTime<Utc>>,
    raw: TripleInput,
}

/// Who gives a topic with a remembered fact (MEM-TOP-1).
fn topic_set_by(who: &MemoryCaller) -> crate::model::TopicSetBy {
    use crate::model::TopicSetBy;
    match who {
        MemoryCaller::Ordin { .. } => TopicSetBy::Ordin,
        MemoryCaller::HostedAgent { .. } => TopicSetBy::HostedAgent,
        MemoryCaller::Person { .. } => TopicSetBy::Person,
        _ => TopicSetBy::Agent,
    }
}

fn fact_invalid(code: &'static str, index: usize, field: &'static str) -> MemoryError {
    MemoryError::Fact { code, index, field }
}

impl Inner {
    /// MEM-TR-3: the 26 seed terms, written the first time a place's vocabulary is read or a fact is
    /// remembered there.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-3
    pub(crate) async fn seed_vocabulary(&self, place: MemoryPlace) -> Result<(), MemoryError> {
        let place_key = store::scope_key(place);
        let seeded = store::count(
            &self.db,
            q("SELECT COUNT(*) AS n FROM memory_terms WHERE place_key = $1 AND origin = 'seed'")
                .text(place_key.clone()),
        )
        .await?;
        if seeded >= SEED_TERMS.len() as u64 {
            return Ok(());
        }
        let mut tx = self.db.begin().await?;
        let now = Timestamp::now();
        for term in &SEED_TERMS {
            tx.exec(
                q(
                    "INSERT INTO memory_terms (place_key, term_key, label, label_norm, meaning, \
                   object_kind, single_valued, origin, agent_instance_id, merged_into, created_at, \
                   renamed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, 'seed', NULL, NULL, $8, NULL) \
                   ON CONFLICT DO NOTHING",
                )
                .text(place_key.clone())
                .text(term.key)
                .text(term.label)
                .text(memory::text::normalise_name(term.label))
                .text(term.meaning)
                .text(term.object.as_str())
                .boolean(term.single_valued)
                .ts(now),
            )
            .await?;
        }
        tx.commit().await
    }

    pub(crate) async fn term(
        &self,
        place: MemoryPlace,
        key: &str,
    ) -> Result<Option<Term>, MemoryError> {
        let row = self
            .db
            .first(
                q(format!(
                    "SELECT {TERM_COLUMNS} FROM memory_terms WHERE place_key = $1 AND term_key = $2"
                ))
                .text(store::scope_key(place))
                .text(key),
            )
            .await?;
        row.as_ref().map(term_of).transpose()
    }

    /// A key → the place's term, a merged term followed once (T1 step 2.2).
    pub(crate) async fn live_term(
        &self,
        place: MemoryPlace,
        key: &str,
    ) -> Result<Option<Term>, MemoryError> {
        match self.term(place, key).await? {
            Some(term) => match &term.merged_into {
                Some(into) => Ok(self
                    .term(place, into)
                    .await?
                    .filter(|t| t.merged_into.is_none())),
                None => Ok(Some(term)),
            },
            None => Ok(None),
        }
    }

    async fn term_by_label(
        &self,
        place: MemoryPlace,
        label: &str,
    ) -> Result<Option<Term>, MemoryError> {
        let row = self
            .db
            .first(
                q(format!(
                    "SELECT {TERM_COLUMNS} FROM memory_terms WHERE place_key = $1 \
                     AND label_norm = $2 AND merged_into IS NULL"
                ))
                .text(store::scope_key(place))
                .text(memory::text::normalise_name(label)),
            )
            .await?;
        row.as_ref().map(term_of).transpose()
    }

    pub(crate) async fn live_terms(&self, place: MemoryPlace) -> Result<Vec<Term>, MemoryError> {
        let rows = self
            .db
            .all(
                q(format!(
                    "SELECT {TERM_COLUMNS} FROM memory_terms WHERE place_key = $1 \
                     AND merged_into IS NULL ORDER BY label_norm, term_key"
                ))
                .text(store::scope_key(place)),
            )
            .await?;
        rows.iter().map(term_of).collect()
    }

    async fn term_count(&self, place: MemoryPlace) -> Result<u64, MemoryError> {
        store::count(
            &self.db,
            q("SELECT COUNT(*) AS n FROM memory_terms WHERE place_key = $1")
                .text(store::scope_key(place)),
        )
        .await
    }

    /// MEM-TR-1: a free key for `label` in the place, `2` to `99` appended to one already used.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-1
    pub(crate) async fn free_key(
        &self,
        place: MemoryPlace,
        base: &str,
        taken: &HashSet<String>,
    ) -> Result<String, MemoryError> {
        let candidates =
            std::iter::once(base.to_owned()).chain((2..=99).map(|n| format!("{base}{n}")));
        for key in candidates {
            if !taken.contains(&key) && self.term(place, &key).await?.is_none() {
                return Ok(key);
            }
        }
        Err(MemoryError::Refused {
            code: "MEM_TOO_MANY_TERMS",
        })
    }

    /// MEM-TR-11: a new word's label and meaning pass the personal-data scan (any span refuses) and
    /// the instruction check.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-11
    pub(crate) async fn word_is_clean(
        &self,
        label: &str,
        meaning: &str,
    ) -> Result<bool, MemoryError> {
        let screened = json!({ "label": label, "meaning": meaning });
        let spans = memory::providers::LocalPiiDetector::new()
            .scan(&screened)
            .await
            .map_err(|_| MemoryError::internal())?;
        if !spans.is_empty() {
            return Ok(false);
        }
        Ok(!memory::write_pipeline::instruction_likelihood(&screened).is_instruction_like)
    }

    pub(crate) async fn insert_term(
        &self,
        tx: &mut crate::db::Tx,
        place: MemoryPlace,
        term: &Term,
    ) -> Result<(), MemoryError> {
        tx.exec(
            q(
                "INSERT INTO memory_terms (place_key, term_key, label, label_norm, meaning, \
               object_kind, single_valued, origin, agent_instance_id, merged_into, created_at, \
               renamed_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULL, $10, NULL)",
            )
            .text(store::scope_key(place))
            .text(term.key.clone())
            .text(term.label.trim().to_owned())
            .text(memory::text::normalise_name(&term.label))
            .text(term.meaning.trim().to_owned())
            .text(term.object.as_str())
            .boolean(term.single_valued)
            .text(term.origin.clone())
            .opt_uuid(term.agent_instance_id.map(|a| a.0))
            .ts(Timestamp::now()),
        )
        .await
        .map(|_| ())
    }

    /// T1 step 2.2 for one proposal, the `index`th fact's.
    #[allow(clippy::too_many_arguments)]
    async fn proposed_term(
        &self,
        place: MemoryPlace,
        index: usize,
        proposal: &TermProposal,
        who: &MemoryCaller,
        pending: &mut Vec<Term>,
        taken: &mut HashSet<String>,
    ) -> Result<Term, MemoryError> {
        if !label_ok(&proposal.label) || !meaning_ok(&proposal.meaning) {
            return Err(MemoryError::invalid("MEM_TERM_PROPOSAL_INVALID", "propose"));
        }
        // MEM-TR-13: checked before a live term of the same label is used, so a kind-of-note word an
        // earlier version kept is not taken up again by a proposal.
        // spec: ordin-desktop/components/MemoryService#MEM-TR-13
        if is_note_word(&proposal.label) {
            return Err(fact_invalid(
                "MEM_TERM_PROPOSAL_INVALID",
                index,
                "predicate.propose.label",
            ));
        }
        if let Some(term) = self.term_by_label(place, &proposal.label).await? {
            return Ok(term);
        }
        let norm = memory::text::normalise_name(&proposal.label);
        if let Some(term) = pending
            .iter()
            .find(|t| memory::text::normalise_name(&t.label) == norm)
        {
            return Ok(term.clone());
        }
        if !self
            .word_is_clean(&proposal.label, &proposal.meaning)
            .await?
        {
            return Err(MemoryError::Refused {
                code: "MEM_TERM_PROPOSAL_REFUSED",
            });
        }
        if self.term_count(place).await? + pending.len() as u64 >= TERMS_MAX {
            return Err(MemoryError::Refused {
                code: "MEM_TOO_MANY_TERMS",
            });
        }
        let key = self
            .free_key(place, &key_of_label(&proposal.label), taken)
            .await?;
        taken.insert(key.clone());
        let (origin, agent) = match who {
            MemoryCaller::CrewMember {
                agent_instance_id, ..
            }
            | MemoryCaller::Agent {
                agent_instance_id, ..
            } => ("agent", Some(*agent_instance_id)),
            MemoryCaller::HostedAgent { .. } => ("hosted_agent", None),
            _ => ("ordin", None),
        };
        let term = Term {
            key,
            label: proposal.label.trim().to_owned(),
            meaning: proposal.meaning.trim().to_owned(),
            object: proposal.object,
            single_valued: proposal.single_valued,
            origin: origin.to_owned(),
            agent_instance_id: agent,
            merged_into: None,
            renamed: false,
        };
        pending.push(term.clone());
        Ok(term)
    }

    /// T1 step 2 for one fact.
    #[allow(clippy::too_many_arguments)]
    async fn check_fact(
        &self,
        place: MemoryPlace,
        who: &MemoryCaller,
        index: usize,
        fact: &TripleInput,
        today: NaiveDate,
        pending: &mut Vec<Term>,
        taken: &mut HashSet<String>,
    ) -> Result<Checked, MemoryError> {
        // 2.1
        if !thing_ok(&fact.subject.thing()) {
            return Err(fact_invalid("MEM_TRIPLE_INVALID", index, "subject"));
        }
        // MEM-TOP-3: a subject's topic is checked with the fact; a refusal refuses the call.
        // spec: ordin-desktop/components/MemoryService#MEM-TOP-3
        if let Some(topic) = &fact.subject.topic {
            if !crate::topics::topic_ok(topic) {
                return Err(fact_invalid("MEM_TOPIC_INVALID", index, "subject.topic"));
            }
            if !self.topic_is_clean(topic).await? {
                return Err(MemoryError::Refused {
                    code: "MEM_TOPIC_REFUSED",
                });
            }
        }
        // 2.2
        let term = match &fact.predicate {
            PredicateInput::Term(key) => {
                let term = match self.live_term(place, key).await? {
                    Some(term) => term,
                    None => match pending.iter().find(|t| &t.key == key) {
                        Some(term) => term.clone(),
                        None => {
                            return Err(MemoryError::Refused {
                                code: "MEM_TERM_UNKNOWN",
                            })
                        }
                    },
                };
                // MEM-TR-13 (d): a kind-of-note word an earlier version kept, or the person renamed
                // to, is refused when used, not only when proposed; the word and its facts stay.
                // spec: ordin-desktop/components/MemoryService#MEM-TR-13
                if is_note_word(&term.label) {
                    return Err(fact_invalid(
                        "MEM_TERM_PROPOSAL_INVALID",
                        index,
                        "predicate",
                    ));
                }
                term
            }
            PredicateInput::Propose { propose } => {
                self.proposed_term(place, index, propose, who, pending, taken)
                    .await?
            }
        };
        // 2.3
        if fact.object.kind() != term.object {
            return Err(MemoryError::Refused {
                code: "MEM_TERM_OBJECT_MISMATCH",
            });
        }
        let object = triple_object(&fact.object)
            .ok_or_else(|| fact_invalid("MEM_TRIPLE_INVALID", index, "object"))?;
        // spec: ordin-desktop/components/MemoryService#MEM-TR-13
        if let ObjectInput::Text(value) = &fact.object {
            if !is_one_claim(value) {
                return Err(fact_invalid("MEM_TRIPLE_INVALID", index, "object.text"));
            }
        }
        // 2.4
        if fact.changed && (!term.single_valued || fact.ended) {
            return Err(MemoryError::Refused {
                code: "MEM_FLAGS_INVALID",
            });
        }
        if fact.confidence_permille.is_some_and(|p| p > 1000) {
            return Err(fact_invalid(
                "MEM_TRIPLE_INVALID",
                index,
                "confidence_permille",
            ));
        }
        let valid_from = match &fact.valid_from {
            Some(value) => Some(
                start_of(value, today)
                    .ok_or_else(|| fact_invalid("MEM_TRIPLE_INVALID", index, "valid_from"))?,
            ),
            None => None,
        };
        let thing_object = match &fact.object {
            ObjectInput::Thing(thing) => Some(thing.clone()),
            _ => None,
        };
        Ok(Checked {
            subject: fact.subject.thing(),
            topic: fact.subject.topic.clone(),
            object,
            thing_object,
            term,
            changed: fact.changed,
            ended: fact.ended,
            confidence: fact.confidence_permille.map(|p| f64::from(p) / 1000.0),
            valid_from,
            raw: fact.clone(),
        })
    }

    /// MEM-TR-5: a live `kept` row of the place with the same subject, term and document and another
    /// version.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-5
    async fn newer_version(
        &self,
        place: MemoryPlace,
        subject_norm: &str,
        term_key: &str,
        source: &FactSource,
    ) -> Result<bool, MemoryError> {
        let n = store::count(
            &self.db,
            q(
                "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE scope_key = $1 \
               AND subject_norm = $2 AND term_key = $3 AND origin_ref = $4 \
               AND origin_version <> $5 AND state = 'kept'",
            )
            .text(store::scope_key(place))
            .text(subject_norm)
            .text(term_key)
            .text(source.origin_ref.clone())
            .text(source.version.clone()),
        )
        .await?;
        Ok(n > 0)
    }

    /// T1.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-4
    pub(crate) async fn remember_facts_as(
        &self,
        who: MemoryCaller,
        agent: Option<AgentInstanceId>,
        input: RememberFactsInput,
        correlation_id: &str,
    ) -> Result<RememberFactsReceipt, MemoryError> {
        self.gate()?;
        // Step 1.
        let run_id: RunId = match who {
            MemoryCaller::Ordin { run_id, .. }
            | MemoryCaller::CrewMember { run_id, .. }
            | MemoryCaller::Agent { run_id, .. }
            | MemoryCaller::HostedAgent { run_id, .. } => run_id,
            MemoryCaller::Person { .. }
            | MemoryCaller::KnowledgeService { .. }
            | MemoryCaller::KnowledgeAgent { .. } => return Err(MemoryError::not_supported()),
        };
        if !(1..=FACTS_MAX).contains(&input.facts.len()) {
            return Err(MemoryError::invalid("MEM_FACTS_INVALID", "facts"));
        }
        // spec: ordin-desktop/components/MemoryService#MEM-TR-6
        if let Some(class) = &input.fact_class {
            if !(1..=128).contains(&class.chars().count()) || class == PASSAGE_CLASS {
                return Err(MemoryError::invalid("MEM_CLASS_INVALID", "fact_class"));
            }
        }
        let document = match (&input.source, &who) {
            (None, MemoryCaller::Ordin { .. }) => {
                return Err(MemoryError::Refused {
                    code: "MEM_SOURCE_REQUIRED",
                })
            }
            (None, _) => None,
            (Some(FactSourceInput::Conversation), MemoryCaller::Ordin { .. }) => None,
            (Some(FactSourceInput::Conversation), _) => {
                return Err(MemoryError::invalid("MEM_SOURCE_INVALID", "source"))
            }
            (Some(FactSourceInput::Document(source)), _) => {
                crate::remember::validate_source(source)?;
                Some(source.clone())
            }
        };
        let place = place_of(&who);
        // Step 2.
        self.seed_vocabulary(place).await?;
        let today = Utc::now().date_naive();
        let mut pending = Vec::new();
        let mut taken = HashSet::new();
        let mut checked = Vec::with_capacity(input.facts.len());
        for (index, fact) in input.facts.iter().enumerate() {
            checked.push(
                self.check_fact(place, &who, index, fact, today, &mut pending, &mut taken)
                    .await?,
            );
        }
        // Step 3.
        let rules = agent.and_then(|a| self.rules_of(a));
        if let Some(rules) = &rules {
            crate::remember::check_class(rules, input.fact_class.as_deref())?;
        }
        self.model_gate()?;
        let mut marker = None;
        if let Some(source) = &document {
            self.screen_source(place, source).await?;
            marker = self
                .ensure_document_source(place, &source.origin_ref, &source.version)
                .await?;
        }
        // The new terms, after every check passed.
        if !pending.is_empty() {
            let mut tx = self.db.begin().await?;
            for term in &pending {
                self.insert_term(&mut tx, place, term).await?;
            }
            tx.commit().await?;
            for term in &pending {
                self.journal(
                    "term_added",
                    Some(run_id),
                    None,
                    match place {
                        MemoryPlace::Workspace(w) => Some(w),
                        MemoryPlace::Scope(_) => None,
                    },
                    "A word was added to the vocabulary.",
                    json!({ "place": store::scope_key(place), "term_key": term.key, "origin": term.origin }),
                )
                .await;
                tracing::info!(target: "ordin::memory", term_key_len = term.key.len(), origin = term.origin.as_str(), "memory.term_added");
            }
        }
        // Step 4.
        let mut items = Vec::with_capacity(checked.len());
        for fact in checked {
            let subject_norm = memory::text::normalise_name(&fact.subject.name);
            let changed = fact.changed
                || match &document {
                    Some(source) if fact.term.single_valued && !fact.ended => {
                        self.newer_version(place, &subject_norm, &fact.term.key, source)
                            .await?
                    }
                    _ => false,
                };
            let content = serde_json::to_value(Triple {
                subject: fact.subject.name.trim().to_owned(),
                predicate: fact.term.key.clone(),
                object: fact.object.clone(),
            })
            .map_err(|_| MemoryError::internal())?;
            let mut things = vec![fact.subject.clone()];
            if let Some(thing) = &fact.thing_object {
                things.push(thing.clone());
            }
            // T1 step 4: the writing agent (`author_of`) is hashed too, so two agents of one run that
            // state the same fact make two writes; a caller with no author keeps the plain form.
            // spec: ordin-desktop/components/MemoryService#MEM-TR-12
            let author = crate::remember::author_of(&who, agent);
            let hashed = match &author {
                Some(author) => {
                    json!({ "fact": fact.raw, "source": input.source, "fact_class": input.fact_class, "author": author })
                }
                None => {
                    json!({ "fact": fact.raw, "source": input.source, "fact_class": input.fact_class })
                }
            };
            let key = format!("{run_id}:facts:{}", sha256_hex(&hashed.to_string()));
            let receipt = self
                .write_triple(TripleWriteArgs {
                    who: &who,
                    agent,
                    place,
                    rules: rules.as_ref(),
                    key,
                    fact_class: input.fact_class.clone(),
                    document: document.as_ref(),
                    marker: marker.clone(),
                    entities: entity_assertions(&things),
                    triple: TripleSeed {
                        content,
                        single_valued: fact.term.single_valued,
                        changed,
                        ended: fact.ended,
                        // The stated value is sent uncapped; an agent's implied link is capped
                        // where the write is built (SA-LINK-01, `agent_implied_link`).
                        confidence: fact.confidence,
                        valid_from: fact.valid_from,
                        subject_norm,
                        term_key: fact.term.key.clone(),
                    },
                    correlation_id,
                })
                .await?;
            // MEM-TOP-3: the subject's thing takes the topic given, when it has none yet.
            if let Some(topic) = &fact.topic {
                self.give_topic(place, &fact.subject.name, topic, topic_set_by(&who))
                    .await;
            }
            items.push(receipt);
        }
        Ok(RememberFactsReceipt { items })
    }

    /// T1 step 4 for one fact: §4 steps 3 to 7 with the triple's body.
    async fn write_triple(
        &self,
        args: TripleWriteArgs<'_>,
    ) -> Result<crate::model::RememberReceipt, MemoryError> {
        let TripleWriteArgs {
            who,
            agent,
            place,
            rules,
            key,
            fact_class,
            document,
            marker,
            entities,
            triple,
            correlation_id,
        } = args;
        let author = crate::remember::author_of(who, agent);
        if let Some(author) = &author {
            self.charge_quota(author)?;
        }
        let (run_id, crew_run_id) = crate::remember::run_of(who);
        let (origin_kind, caller_kind) = crate::remember::origin_of(who, document.is_some());
        let mut seed = RowSeed::of(origin_kind, caller_kind);
        seed.agent_instance_id = agent_of(who);
        seed.run_id = run_id;
        seed.crew_run_id = crew_run_id;
        seed.author = author;
        if origin_kind == "agent" {
            seed.gate_trust = Some(crate::remember::AGENT_UNSOURCED_TRUST);
        }
        self.learning_gate(who, &mut seed, false)?;
        if let Some(source) = document {
            seed.origin_ref = Some(source.origin_ref.clone());
            seed.origin_version = Some(source.version.clone());
        }
        seed.retain_until = rules
            .and_then(|r| r.max_age_days)
            .map(|days| Timestamp(Timestamp::now().0 + chrono::Duration::days(i64::from(days))));
        let retains = rules.map(|r| r.retains.as_slice());
        // An agent's implied link counts for at most 0.85 (SA-LINK-01): the write states the link
        // itself, which the component takes as the implied one (memory SA-TRIPLE-04); the fact keeps
        // its stated confidence, so the gate reads it uncapped (SA-TRUST-02).
        // spec: ordin-desktop/components/MemoryService#MEM-TR-9
        let links = agent_implied_link(seed.author.is_some(), &triple);
        let write = Write {
            place,
            context: self.caller_context(who, retains),
            key,
            statement: String::new(),
            about: None,
            memory_class: "episodic",
            fact_class,
            source: document.map(|s| (s.origin_ref.clone(), marker)),
            visibility: "team-shared",
            collection: None,
            entities,
            links,
            made_from: Vec::new(),
            row: seed,
            triple: Some(triple),
        };
        self.write(write, correlation_id).await
    }

    /// Rule FV′: the triple of a fact's content, its predicate's label from the place's vocabulary.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-7
    pub(crate) async fn triple_view(
        &self,
        fact: &Fact,
        place: Option<MemoryPlace>,
        things: &[ThingRef],
    ) -> Result<Option<TripleView>, MemoryError> {
        let Ok(triple) = serde_json::from_value::<Triple>(fact.content.clone()) else {
            return Ok(None);
        };
        let by_name = |name: &str| {
            let norm = memory::text::normalise_name(name);
            // The id of the fact's first entity of that name; the name as the fact states it.
            ThingRef {
                id: things
                    .iter()
                    .find(|t| memory::text::normalise_name(&t.name) == norm)
                    .map_or_else(|| MemoryThingId(String::new()), |t| t.id.clone()),
                name: name.to_owned(),
            }
        };
        let label = match place {
            Some(place) => self
                .term(place, &triple.predicate)
                .await?
                .map_or_else(|| triple.predicate.clone(), |t| t.label),
            None => triple.predicate.clone(),
        };
        let object = match &triple.object {
            TripleObject::Entity { name } => ObjectView::Thing {
                thing: by_name(name),
            },
            TripleObject::Text { value } => ObjectView::Text {
                value: value.clone(),
            },
            TripleObject::Number { value, unit } => ObjectView::Number {
                value: value.clone(),
                unit: unit.clone(),
            },
            TripleObject::Money { amount, currency } => ObjectView::Money {
                amount: amount.clone(),
                currency: currency.clone(),
            },
            TripleObject::Date { value } => ObjectView::Date {
                value: value.clone(),
            },
            TripleObject::Boolean { value } => ObjectView::YesNo { value: *value },
        };
        Ok(Some(TripleView {
            subject: by_name(&triple.subject),
            predicate: TermRef {
                key: triple.predicate.clone(),
                label,
            },
            object,
        }))
    }

    /// `conflicts_with` for a page of views: one `Service::fact_conflicts` per 100 facts, ids mapped
    /// through `memory_fact_refs`.
    // spec: ordin-desktop/components/MemoryService#M-10
    pub(crate) async fn conflicts_by_fact(
        &self,
        place: MemoryPlace,
        facts: &[String],
    ) -> Result<HashMap<String, Vec<String>>, MemoryError> {
        let mut out = HashMap::new();
        let ids: Vec<String> = facts
            .iter()
            .filter(|f| f.starts_with("fact:"))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        for chunk in ids.chunks(100) {
            let guard = self.auth.mint(self.place_context(place)).await?;
            let correlation_id = new_correlation_id();
            let result = self
                .service
                .fact_conflicts(
                    guard.cx(&correlation_id, None),
                    FactConflictsRequest {
                        ids: chunk.to_vec(),
                    },
                )
                .await
                .map_err(crate::error::map_call_error)?
                .data;
            for item in result.items {
                if !item.with.is_empty() {
                    out.insert(item.fact, item.with);
                }
            }
        }
        Ok(out)
    }

    /// Component ids → row ids, an id with no row skipped.
    pub(crate) async fn row_ids_of(
        &self,
        facts: &[String],
    ) -> Result<HashMap<String, MemoryFactId>, MemoryError> {
        Ok(store::refs_by_facts(&self.db, facts)
            .await?
            .into_iter()
            .map(|r| (r.memory_fact_id, r.id))
            .collect())
    }

    /// Fill `conflicts_with` on views of `facts` (same order), for one place.
    pub(crate) async fn fill_conflicts(
        &self,
        place: MemoryPlace,
        facts: &[&Fact],
        views: &mut [FactView],
    ) -> Result<(), MemoryError> {
        let ids: Vec<String> = facts.iter().map(|f| f.id.clone()).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let conflicts = self.conflicts_by_fact(place, &ids).await?;
        if conflicts.is_empty() {
            return Ok(());
        }
        let others: Vec<String> = conflicts.values().flatten().cloned().collect();
        let rows = self.row_ids_of(&others).await?;
        for (fact, view) in facts.iter().zip(views.iter_mut()) {
            if let Some(with) = conflicts.get(&fact.id) {
                view.conflicts_with = with
                    .iter()
                    .filter_map(|w| rows.get(w).copied())
                    .take(8)
                    .collect();
            }
        }
        Ok(())
    }
}

/// The arguments of one triple write.
struct TripleWriteArgs<'a> {
    who: &'a MemoryCaller,
    agent: Option<AgentInstanceId>,
    place: MemoryPlace,
    rules: Option<&'a crate::model::AgentMemoryRules>,
    key: String,
    fact_class: Option<String>,
    document: Option<&'a FactSource>,
    marker: Option<String>,
    entities: Vec<memory::types::api::EntityAssertion>,
    triple: TripleSeed,
    correlation_id: &'a str,
}

/// The `statement` of a fact with a triple (rule FV′).
pub(crate) fn triple_statement(view: &TripleView) -> String {
    format!(
        "{} · {} · {}",
        view.subject.name,
        view.predicate.label,
        object_text(&view.object)
    )
}

fn place_key_of(place: MemoryPlace) -> String {
    store::scope_key(place)
}

impl MemoryService {
    /// 1–16 facts sharing one source; each becomes its own fact (ADR-118 item 9).
    pub async fn remember_facts(
        &self,
        who: MemoryCaller,
        input: RememberFactsInput,
    ) -> Result<RememberFactsReceipt, MemoryError> {
        let agent = agent_of(&who);
        self.inner
            .remember_facts_as(who, agent, input, &new_correlation_id())
            .await
    }

    /// V1.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-3
    pub async fn vocabulary(&self, place: MemoryPlace) -> Result<VocabularyView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.seed_vocabulary(place).await?;
        let counts: HashMap<String, u32> = inner
            .db
            .all(
                q(
                    "SELECT term_key, COUNT(*) AS n FROM memory_fact_refs WHERE scope_key = $1 \
                   AND state = 'kept' AND term_key IS NOT NULL GROUP BY term_key",
                )
                .text(place_key_of(place)),
            )
            .await?
            .iter()
            .map(|r| Ok((r.text("term_key")?, r.int("n")? as u32)))
            .collect::<Result<_, MemoryError>>()?;
        let terms = inner
            .live_terms(place)
            .await?
            .iter()
            .map(|t| t.view(counts.get(&t.key).copied().unwrap_or(0)))
            .collect();
        Ok(VocabularyView { terms })
    }

    /// V2.
    pub async fn rename_term(
        &self,
        place: MemoryPlace,
        key: String,
        label: String,
    ) -> Result<TermView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.seed_vocabulary(place).await?;
        let term = inner
            .term(place, &key)
            .await?
            .filter(|t| t.merged_into.is_none())
            .ok_or(MemoryError::NotFound {
                code: "MEM_TERM_NOT_FOUND",
            })?;
        if !label_ok(&label) {
            return Err(MemoryError::invalid("MEM_TERM_PROPOSAL_INVALID", "label"));
        }
        if let Some(other) = inner.term_by_label(place, &label).await? {
            if other.key != term.key {
                return Err(MemoryError::Refused {
                    code: "MEM_TERM_LABEL_TAKEN",
                });
            }
        }
        inner
            .db
            .exec(
                q(
                    "UPDATE memory_terms SET label = $1, label_norm = $2, renamed_at = $3 \
                   WHERE place_key = $4 AND term_key = $5",
                )
                .text(label.trim().to_owned())
                .text(memory::text::normalise_name(&label))
                .ts(Timestamp::now())
                .text(place_key_of(place))
                .text(key.clone()),
            )
            .await?;
        inner
            .journal(
                "term_renamed",
                None,
                None,
                workspace_of(place),
                "A word was renamed.",
                json!({ "place": place_key_of(place), "term_key": key }),
            )
            .await;
        tracing::info!(target: "ordin::memory", term_key_len = key.len(), "memory.term_renamed");
        let term = inner
            .term(place, &key)
            .await?
            .ok_or_else(MemoryError::internal)?;
        let count = inner.kept_with_term(place, &key).await?;
        Ok(term.view(count as u32))
    }

    /// V3.
    pub async fn merge_term(
        &self,
        place: MemoryPlace,
        key: String,
        into: String,
    ) -> Result<MergeTermReceipt, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if key == into {
            return Err(MemoryError::Refused {
                code: "MEM_SAME_TERM",
            });
        }
        inner.seed_vocabulary(place).await?;
        let live = |t: Option<Term>| {
            t.filter(|t| t.merged_into.is_none())
                .ok_or(MemoryError::NotFound {
                    code: "MEM_TERM_NOT_FOUND",
                })
        };
        let from = live(inner.term(place, &key).await?)?;
        let kept = live(inner.term(place, &into).await?)?;
        if from.object != kept.object {
            return Err(MemoryError::Refused {
                code: "MEM_TERM_KIND_DIFFERS",
            });
        }
        if inner.kept_with_term(place, &key).await? > MERGE_FACTS_MAX {
            return Err(MemoryError::Refused {
                code: "MEM_TERM_TOO_MANY_FACTS",
            });
        }
        inner.model_gate()?;
        let rows = inner.kept_rows_with_term(place, &key).await?;
        inner
            .db
            .exec(
                q("UPDATE memory_terms SET merged_into = $1 WHERE place_key = $2 AND term_key = $3")
                    .text(into.clone())
                    .text(place_key_of(place))
                    .text(key.clone()),
            )
            .await?;
        let mut rewritten = 0u32;
        for row in &rows {
            let fact = inner.stored_fact(row).await?;
            let Ok(mut triple) = serde_json::from_value::<Triple>(fact.content.clone()) else {
                continue;
            };
            triple.predicate = into.clone();
            let mut seed = RowSeed::of("person", "person");
            seed.origin_kind = leak_origin(&row.origin_kind);
            seed.caller_kind = row.caller_kind.as_deref().map_or("person", leak_origin);
            seed.origin_ref.clone_from(&row.origin_ref);
            seed.origin_version.clone_from(&row.origin_version);
            seed.agent_instance_id = row.agent_instance_id;
            seed.run_id = row.run_id;
            seed.replaces_id = Some(row.id);
            seed.retain_until = row.retain_until;
            seed.everywhere = row.everywhere;
            let content = serde_json::to_value(&triple).map_err(|_| MemoryError::internal())?;
            inner
                .write(
                    Write {
                        place,
                        context: inner.place_context(place),
                        key: format!("merge:{}:{into}", row.id),
                        statement: String::new(),
                        about: None,
                        memory_class: "semantic",
                        fact_class: row.fact_class.clone(),
                        source: None,
                        visibility: if row.everywhere {
                            "tenant-shared"
                        } else {
                            "team-shared"
                        },
                        collection: None,
                        entities: Vec::new(),
                        links: Vec::new(),
                        made_from: Vec::new(),
                        row: seed,
                        triple: Some(TripleSeed {
                            content,
                            single_valued: kept.single_valued,
                            changed: false,
                            ended: false,
                            confidence: None,
                            valid_from: None,
                            subject_norm: row
                                .subject_norm
                                .clone()
                                .unwrap_or_else(|| memory::text::normalise_name(&triple.subject)),
                            term_key: into.clone(),
                        }),
                    },
                    &new_correlation_id(),
                )
                .await?;
            rewritten += 1;
        }
        inner
            .journal(
                "term_merged",
                None,
                None,
                workspace_of(place),
                "Two words were merged.",
                json!({ "place": place_key_of(place), "term_key": key, "into": into, "rewritten": rewritten }),
            )
            .await;
        tracing::info!(target: "ordin::memory", term_key_len = key.len(), into_len = into.len(), rewritten, "memory.term_merged");
        let count = inner.kept_with_term(place, &into).await?;
        Ok(MergeTermReceipt {
            kept: kept.view(count as u32),
            rewritten,
        })
    }

    /// C1.
    // spec: ordin-desktop/components/MemoryService#M-10
    pub async fn keep_conflict_side(
        &self,
        keep: MemoryFactId,
        other: MemoryFactId,
    ) -> Result<FactView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let keep_row = inner.row(keep).await?;
        let other_row = inner.row(other).await?;
        if keep_row.state != FactState::Kept
            || other_row.state != FactState::Kept
            || keep_row.scope_key != other_row.scope_key
        {
            return Err(MemoryError::fact_not_found());
        }
        let place = keep_row.place()?;
        let listed = inner
            .conflicts_by_fact(place, std::slice::from_ref(&keep_row.memory_fact_id))
            .await?;
        if !listed
            .get(&keep_row.memory_fact_id)
            .is_some_and(|w| w.contains(&other_row.memory_fact_id))
        {
            return Err(MemoryError::Refused {
                code: "MEM_CONFLICT_NOT_FOUND",
            });
        }
        inner
            .retire_fact(
                place,
                &other_row.memory_fact_id,
                &format!("conflict:{keep}:{other}"),
            )
            .await?;
        store::set_state(
            &inner.db,
            other_row.id,
            FactState::Retired,
            Some(Timestamp::now()),
        )
        .await?;
        inner
            .journal(
                "conflict_settled",
                None,
                None,
                keep_row.workspace_id,
                "The person kept one of two facts that disagreed.",
                json!({ "kept": keep.to_string(), "closed": other.to_string() }),
            )
            .await;
        tracing::info!(target: "ordin::memory", kept = keep.to_string().as_str(), closed = other.to_string().as_str(), "memory.conflict_settled");
        let fact = inner.stored_fact(&keep_row).await?;
        inner.view_of_row(&fact, &keep_row).await
    }

    /// P1.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-6
    pub async fn passages(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
    ) -> Result<PassagePage, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        let place = MemoryPlace::Workspace(workspace_id);
        let prefix = format!("{PAGE_ORIGIN}{page_id}#");
        let mut rows: Vec<(u32, RefRow)> =
            store::rows_with_prefix(&inner.db, &store::scope_key(place), &prefix, 10_000)
                .await?
                .into_iter()
                .filter(|r| r.state == FactState::Kept)
                .filter_map(|r| {
                    let n = r
                        .origin_ref
                        .as_deref()?
                        .rsplit_once('#')?
                        .1
                        .parse::<u32>()
                        .ok()?;
                    Some((n, r))
                })
                .collect();
        rows.sort_by_key(|(n, r)| (*n, r.written_at.0));
        rows.truncate(200);
        let mut items = Vec::with_capacity(rows.len());
        for (_, row) in rows {
            let Some(fact) = inner.fact_of(&row).await? else {
                continue;
            };
            items.push(PassageView {
                id: row.id,
                heading: crate::views::text_of(&fact, "subject"),
                text: crate::views::text_of(&fact, "statement").unwrap_or_default(),
                version: row.origin_version.clone().unwrap_or_default(),
                written_at: row.written_at,
            });
        }
        Ok(PassagePage { items })
    }

    /// G1 (ADR-120; MEM-TR-10).
    // spec: ordin-desktop/components/MemoryService#MEM-TR-10
    pub async fn diagram(
        &self,
        workspace_id: WorkspaceId,
        centre: Option<MemoryThingId>,
    ) -> Result<DiagramView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        let place = MemoryPlace::Workspace(workspace_id);
        // Step 1.
        let rows = inner
            .db
            .all(
                q("SELECT f.id AS id, f.subject_entity AS subject_entity, f.predicate AS predicate, \
                   f.object_kind AS object_kind, f.object_value AS object_value, \
                   f.object_unit AS object_unit, f.object_entity AS object_entity, \
                   r.id AS row_id \
                   FROM mem_fact f JOIN memory_fact_refs r ON r.memory_fact_id = f.id \
                   WHERE r.state = 'kept' AND f.valid_to IS NULL AND f.predicate IS NOT NULL \
                   AND (f.fact_class IS NULL OR f.fact_class <> 'passage') \
                   AND (f.owner_team = $1 OR r.everywhere = $2)")
                .text(place_key_of(place))
                .boolean(true),
            )
            .await?;
        struct Considered {
            fact: String,
            row: MemoryFactId,
            subject: String,
            predicate: String,
            kind: String,
            value: String,
            unit: Option<String>,
            object: Option<String>,
        }
        let mut facts = Vec::with_capacity(rows.len());
        for r in &rows {
            facts.push(Considered {
                fact: r.text("id")?,
                row: MemoryFactId(r.uuid("row_id")?),
                subject: r.opt_text("subject_entity")?.unwrap_or_default(),
                predicate: r.text("predicate")?,
                kind: r.opt_text("object_kind")?.unwrap_or_default(),
                value: r.opt_text("object_value")?.unwrap_or_default(),
                unit: r.opt_text("object_unit")?,
                object: r.opt_text("object_entity")?,
            });
        }
        let mut named: HashMap<&str, u32> = HashMap::new();
        for f in &facts {
            *named.entry(f.subject.as_str()).or_default() += 1;
            if let Some(o) = &f.object {
                *named.entry(o.as_str()).or_default() += 1;
            }
        }
        let all_ids: Vec<String> = named.keys().map(|k| (*k).to_owned()).collect();
        let names = inner.entity_names(&all_ids).await?;
        let rank = |id: &str| {
            (
                std::cmp::Reverse(named.get(id).copied().unwrap_or(0)),
                names.get(id).cloned().unwrap_or_default(),
            )
        };
        // Steps 2 and 3.
        let mut steps: BTreeMap<String, u8> = BTreeMap::new();
        let (drawn, total) = match &centre {
            Some(centre) => {
                let c = centre.0.as_str();
                if !named.contains_key(c) {
                    return Err(MemoryError::thing_not_found());
                }
                let mut neighbours: HashMap<&str, HashSet<&str>> = HashMap::new();
                for f in facts.iter().filter(|f| f.kind == "entity") {
                    if let Some(o) = &f.object {
                        neighbours
                            .entry(f.subject.as_str())
                            .or_default()
                            .insert(o.as_str());
                        neighbours
                            .entry(o.as_str())
                            .or_default()
                            .insert(f.subject.as_str());
                    }
                }
                let mut reached: Vec<Vec<String>> = vec![vec![c.to_owned()]];
                let mut seen: HashSet<String> = HashSet::from([c.to_owned()]);
                for _ in 1..=2 {
                    let mut next: Vec<String> = Vec::new();
                    for from in reached.last().into_iter().flatten() {
                        for to in neighbours.get(from.as_str()).into_iter().flatten() {
                            if seen.insert((*to).to_owned()) {
                                next.push((*to).to_owned());
                            }
                        }
                    }
                    next.sort_by_key(|id| rank(id));
                    reached.push(next);
                }
                let total = reached.iter().map(Vec::len).sum::<usize>() as u32;
                let mut drawn = Vec::new();
                for (step, ids) in reached.iter().enumerate() {
                    for id in ids {
                        if drawn.len() < DIAGRAM_THINGS {
                            steps.insert(id.clone(), step as u8);
                            drawn.push(id.clone());
                        }
                    }
                }
                (drawn, total)
            }
            None => {
                let mut ids = all_ids.clone();
                ids.sort_by_key(|id| rank(id));
                let total = ids.len() as u32;
                ids.truncate(DIAGRAM_THINGS);
                (ids, total)
            }
        };
        let drawn_set: HashSet<&str> = drawn.iter().map(String::as_str).collect();
        let position: HashMap<&str, usize> = drawn
            .iter()
            .enumerate()
            .map(|(i, id)| (id.as_str(), i))
            .collect();
        let terms: HashMap<String, Term> = inner
            .live_terms(place)
            .await?
            .into_iter()
            .map(|t| (t.key.clone(), t))
            .collect();
        let word = |key: &str| TermRef {
            key: key.to_owned(),
            label: terms
                .get(key)
                .map_or_else(|| key.to_owned(), |t| t.label.clone()),
        };
        // Step 4.
        let mut join_facts: Vec<&Considered> = facts
            .iter()
            .filter(|f| {
                f.kind == "entity"
                    && drawn_set.contains(f.subject.as_str())
                    && f.object.as_deref().is_some_and(|o| drawn_set.contains(o))
            })
            .collect();
        join_facts.sort_by_key(|f| {
            (
                position.get(f.subject.as_str()).copied(),
                f.predicate.clone(),
            )
        });
        join_facts.truncate(DIAGRAM_JOINS);
        let mut value_facts: Vec<&Considered> = match &centre {
            Some(centre) => facts
                .iter()
                .filter(|f| f.kind != "entity" && f.subject == centre.0)
                .collect(),
            None => Vec::new(),
        };
        value_facts.sort_by_key(|f| word(&f.predicate).label);
        value_facts.truncate(DIAGRAM_VALUES);
        // Step 5.
        let drawn_facts: Vec<String> = join_facts
            .iter()
            .chain(value_facts.iter())
            .map(|f| f.fact.clone())
            .collect();
        let conflicts = inner.conflicts_by_fact(place, &drawn_facts).await?;
        let drawn_fact_set: HashSet<&String> = drawn_facts.iter().collect();
        let row_of: HashMap<&str, MemoryFactId> =
            facts.iter().map(|f| (f.fact.as_str(), f.row)).collect();
        let mut pairs: BTreeSet<(MemoryFactId, MemoryFactId)> = BTreeSet::new();
        for (fact, with) in &conflicts {
            for w in with {
                if drawn_fact_set.contains(w) {
                    if let (Some(a), Some(b)) = (row_of.get(fact.as_str()), row_of.get(w.as_str()))
                    {
                        pairs.insert(if a < b { (*a, *b) } else { (*b, *a) });
                    }
                }
            }
        }
        let thing = |id: &str| ThingRef {
            id: MemoryThingId(id.to_owned()),
            name: names.get(id).cloned().unwrap_or_default(),
        };
        let view = DiagramView {
            centre: centre.as_ref().map(|c| thing(&c.0)),
            things: drawn
                .iter()
                .map(|id| DiagramThing {
                    thing: thing(id),
                    step: steps.get(id).copied(),
                    fact_count: named.get(id.as_str()).copied().unwrap_or(0),
                })
                .collect(),
            joins: join_facts
                .iter()
                .map(|f| DiagramJoin {
                    fact_id: f.row,
                    from: MemoryThingId(f.subject.clone()),
                    to: MemoryThingId(f.object.clone().unwrap_or_default()),
                    word: word(&f.predicate),
                    conflict: conflicts.contains_key(&f.fact),
                })
                .collect(),
            values: value_facts
                .iter()
                .map(|f| DiagramValue {
                    fact_id: f.row,
                    word: word(&f.predicate),
                    object: value_view(&f.kind, &f.value, f.unit.as_deref()),
                    conflict: conflicts.contains_key(&f.fact),
                })
                .collect(),
            conflicts: pairs.into_iter().collect(),
            left_out: total.saturating_sub(drawn.len() as u32),
            total_things: total,
        };
        tracing::info!(
            target: "ordin::memory",
            things = view.things.len(),
            joins = view.joins.len(),
            values = view.values.len(),
            left_out = view.left_out,
            "memory.diagram"
        );
        Ok(view)
    }
}

/// A stored value's view (G1 step 4).
fn value_view(kind: &str, value: &str, unit: Option<&str>) -> ObjectView {
    match kind {
        "number" => ObjectView::Number {
            value: value.to_owned(),
            unit: unit.map(str::to_owned),
        },
        "money" => ObjectView::Money {
            amount: value.to_owned(),
            currency: unit.unwrap_or_default().to_owned(),
        },
        "date" => ObjectView::Date {
            value: value.to_owned(),
        },
        "boolean" => ObjectView::YesNo {
            value: value == "true",
        },
        _ => ObjectView::Text {
            value: value.to_owned(),
        },
    }
}

fn workspace_of(place: MemoryPlace) -> Option<WorkspaceId> {
    match place {
        MemoryPlace::Workspace(w) => Some(w),
        MemoryPlace::Scope(_) => None,
    }
}

/// A stored origin or caller kind as the `&'static str` a `RowSeed` holds.
fn leak_origin(kind: &str) -> &'static str {
    const KINDS: [&str; 10] = [
        "conversation",
        "agent",
        "document",
        "knowledge_page",
        "person",
        "team",
        "ordin",
        "crew_member",
        "hosted_agent",
        "knowledge_service",
    ];
    KINDS
        .iter()
        .find(|k| **k == kind)
        .copied()
        .unwrap_or("person")
}

impl Inner {
    /// Live `kept` rows of the place with `term_key`.
    pub(crate) async fn kept_with_term(
        &self,
        place: MemoryPlace,
        key: &str,
    ) -> Result<u64, MemoryError> {
        store::count(
            &self.db,
            q(
                "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE scope_key = $1 \
               AND term_key = $2 AND state = 'kept'",
            )
            .text(place_key_of(place))
            .text(key),
        )
        .await
    }

    async fn kept_rows_with_term(
        &self,
        place: MemoryPlace,
        key: &str,
    ) -> Result<Vec<RefRow>, MemoryError> {
        let ids: Vec<MemoryFactId> = self
            .db
            .all(
                q(
                    "SELECT id FROM memory_fact_refs WHERE scope_key = $1 AND term_key = $2 \
                   AND state = 'kept' ORDER BY written_at",
                )
                .text(place_key_of(place))
                .text(key),
            )
            .await?
            .iter()
            .map(|r| Ok(MemoryFactId(r.uuid("id")?)))
            .collect::<Result<_, MemoryError>>()?;
        store::refs_by_ids(&self.db, &ids).await
    }

    /// `mem_entity.canonical_name` of `ids`, one parameterised `SELECT`.
    pub(crate) async fn entity_names(
        &self,
        ids: &[String],
    ) -> Result<HashMap<String, String>, MemoryError> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(500) {
            let mut st = q(format!(
                "SELECT id, canonical_name FROM mem_entity WHERE id IN ({})",
                placeholders(1, chunk.len())
            ));
            for id in chunk {
                st = st.text(id.clone());
            }
            for r in self.db.all(st).await? {
                out.insert(r.text("id")?, r.text("canonical_name")?);
            }
        }
        Ok(out)
    }
}

/// The implied link an agent's write states with its confidence capped (SA-LINK-01): only for an
/// entity object and a write with an author; none otherwise.
// spec: ordin-desktop/components/MemoryService#SA-LINK-01
fn agent_implied_link(
    authored: bool,
    triple: &crate::remember::TripleSeed,
) -> Vec<memory::types::api::LinkAssertion> {
    let c = &triple.content;
    let (Some(subject), Some(predicate), Some(name)) = (
        c.get("subject").and_then(|v| v.as_str()),
        c.get("predicate").and_then(|v| v.as_str()),
        c.pointer("/object/name").and_then(|v| v.as_str()),
    ) else {
        return Vec::new();
    };
    if !authored || c.pointer("/object/kind").and_then(|v| v.as_str()) != Some("entity") {
        return Vec::new();
    }
    vec![memory::types::api::LinkAssertion {
        from: subject.to_owned(),
        kind: predicate.to_owned(),
        to: name.to_owned(),
        ended: triple.ended,
        confidence: Some(
            triple
                .confidence
                .unwrap_or(1.0)
                .min(crate::remember::AGENT_LINK_CONFIDENCE_CAP),
        ),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: ordin-desktop/components/MemoryService#MEM-TR-1
    #[test]
    fn keys_derive_from_labels() {
        assert_eq!(key_of_label("account manager"), "accountManager");
        assert_eq!(key_of_label("3D printer"), "t3dPrinter");
        assert_eq!(key_of_label("  Works  For "), "worksFor");
        let hashed = key_of_label("日本語");
        assert!(hashed.starts_with('t') && hashed.len() == 11, "{hashed}");
        assert!(key_of_label(&"a".repeat(80)).chars().count() <= 62);
        let re_ok = |k: &str| {
            k.starts_with(|c: char| c.is_ascii_lowercase())
                && k.chars().all(|c| c.is_ascii_alphanumeric())
        };
        for label in ["account manager", "3D printer", "日本語", "x-y_z"] {
            assert!(re_ok(&key_of_label(label)), "{label}");
        }
    }

    // spec: ordin-desktop/components/MemoryService#MEM-TR-9
    #[test]
    fn a_stated_start_is_its_first_day() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        assert_eq!(
            start_of("2025-04", today)
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "2025-04-01T00:00:00.000Z"
        );
        assert!(start_of("2026", today).is_some());
        assert!(start_of("2026-01-15", today).is_some());
        assert!(start_of("2026-01-16", today).is_none());
        assert!(start_of("2026-02", today).is_none());
        assert!(start_of("2027", today).is_none());
        assert!(start_of("2025-02-30", today).is_none());
        assert!(start_of("25", today).is_none());
    }

    #[test]
    fn the_seed_has_26_distinct_terms() {
        let keys: HashSet<&str> = SEED_TERMS.iter().map(|t| t.key).collect();
        assert_eq!(keys.len(), 26);
        assert!(
            !keys.contains("email") && !keys.contains("telephone") && !keys.contains("address")
        );
        for t in &SEED_TERMS {
            assert!(label_ok(t.label) && meaning_ok(t.meaning), "{}", t.key);
        }
    }

    #[test]
    fn objects_are_checked_by_kind() {
        assert!(triple_object(&ObjectInput::Money {
            amount: "12.50".into(),
            currency: "gbp".into()
        })
        .is_none());
        assert!(triple_object(&ObjectInput::Date("1998-02-30".into())).is_none());
        assert!(triple_object(&ObjectInput::Number {
            value: "240".into(),
            unit: Some("people".into())
        })
        .is_some());
        assert!(kind_matches(
            ObjectKindInput::YesNo,
            &TripleObject::Boolean { value: true }
        ));
    }
}
