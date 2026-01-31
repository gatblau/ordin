//! `remember` (*Internal Logic* §4 with the ADR-111 and ADR-112/113 changes), and the write every
//! other path shares: corrections, widenings, page sections and knowledge documents.

use chrono::Duration as ChronoDuration;
use memory::error::AppError;
use memory::service::CallError;
use memory::types::api::{EntityAssertion, JobAckStatus, LinkAssertion};
use memory::types::domain::{Fact, Source};
use memory::types::scope::{ScopeContext, ScopeRef};
use memory::write_pipeline::source_id;
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::{AgentInstanceId, CrewRunId, MemoryFactId, RunId, TeamFolderId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::error::{map_remember_error, MemoryError};
use crate::model::{
    FactSource, FactState, LinkInput, MemoryCaller, MemoryPlace, RememberInput, RememberReceipt,
    ThingInput,
};
use crate::service::{new_correlation_id, place_of, store_error_of, Inner, MemoryService};
use crate::store::{self, RefRow};

/// The component's idempotency key limit (`mem_idempotency_record`).
const KEY_MAX: usize = 255;

/// The trust of a document's source when a fact citing it is written: with `confidence = 0.5 + 0.5 ×
/// trust`, the gate scores `0.3 + 0.7 × trust`, 0.72 here against `trust_admit` 0.7.
// spec: ordin-desktop/components/MemoryService#SA-TRUST-01
pub(crate) const DOCUMENT_TRUST: f64 = 0.6;

/// What the gate reads for a write an agent makes with no source: 0.65, which holds it for a person.
// spec: ordin-desktop/components/MemoryService#SA-TRUST-02
pub(crate) const AGENT_UNSOURCED_TRUST: f64 = 0.5;

/// The most an agent's stated link confidence counts for, as a colleague's does (SA-TRUST-03).
// spec: ordin-desktop/components/MemoryService#SA-LINK-01
pub(crate) const AGENT_LINK_CONFIDENCE_CAP: f64 = 0.85;

/// What a page section's origin begins with. Only the page path writes one.
pub(crate) const PAGE_ORIGIN: &str = "knowledge-page:";

pub(crate) fn sha256_hex(text: &str) -> String {
    ordin_types::num::Sha256Hex::from_digest(&Sha256::digest(text.as_bytes()).into()).0
}

/// A key of *Internal Logic* §4 step 3, within the component's 255 characters.
// The page key `page:<page_id>:<version>:<n>` holds a version of up to 256 characters and
// the document key `kdoc:<origin_prefix>:<version>:<n>` a prefix of up to 2,048; the component refuses
// a key over 255, which §4 step 5 would report as MEM_INTERNAL. A key over the limit is sent as its
// digest; a shorter one keeps the spec's form.
pub(crate) fn bounded_key(key: String) -> String {
    if key.chars().count() <= KEY_MAX {
        key
    } else {
        format!("sha256:{}", sha256_hex(&key))
    }
}

/// The fact id the write pipeline derives for the first fact of a job.
// §4 step 6 gives `fact:<UUIDv5(NAMESPACE_DNS, "fact:<key>:0")>`; the changed pipeline
// seeds the id with the owner too (`write_pipeline/mod.rs`, `owner_seed`), so that two owners of one
// key get two facts. The derivation is private to the pipeline, so its form is repeated here. The
// happy-path scenarios fail if the two ever differ.
pub(crate) fn pipeline_fact_id(owner: &ScopeRef, key: &str) -> String {
    let seed = json!([
        "fact",
        owner.tenant,
        owner.team,
        owner.user,
        format!("{key}:0")
    ])
    .to_string();
    format!(
        "fact:{}",
        Uuid::new_v5(&Uuid::NAMESPACE_DNS, seed.as_bytes())
    )
}

/// What a write's `memory_fact_refs` row carries beyond the common columns.
#[derive(Debug, Clone)]
pub(crate) struct RowSeed {
    pub(crate) origin_kind: &'static str,
    pub(crate) caller_kind: &'static str,
    pub(crate) agent_instance_id: Option<AgentInstanceId>,
    pub(crate) run_id: Option<RunId>,
    pub(crate) crew_run_id: Option<CrewRunId>,
    pub(crate) origin_ref: Option<String>,
    pub(crate) origin_version: Option<String>,
    pub(crate) everywhere: bool,
    pub(crate) replaces_id: Option<MemoryFactId>,
    pub(crate) retain_until: Option<Timestamp>,
    pub(crate) read_folder_id: Option<TeamFolderId>,
    pub(crate) may_be_out_of_date: bool,
    pub(crate) page_status: Option<PageStatus>,
    pub(crate) page_tier: Option<PageTier>,
    /// What the gate reads in place of the source's trust.
    pub(crate) gate_trust: Option<f64>,
    /// Which of the place's callers writes (`Relationship.author`); `None` for ordin and a person.
    pub(crate) author: Option<String>,
}

impl RowSeed {
    pub(crate) fn of(origin_kind: &'static str, caller_kind: &'static str) -> RowSeed {
        RowSeed {
            origin_kind,
            caller_kind,
            agent_instance_id: None,
            run_id: None,
            crew_run_id: None,
            origin_ref: None,
            origin_version: None,
            everywhere: false,
            replaces_id: None,
            retain_until: None,
            read_folder_id: None,
            may_be_out_of_date: false,
            page_status: None,
            page_tier: None,
            gate_trust: None,
            author: None,
        }
    }
}

/// One write through `Service::remember` (§4 steps 3 to 7).
pub(crate) struct Write {
    pub(crate) place: MemoryPlace,
    pub(crate) context: ScopeContext,
    /// §4 step 3, before `bounded_key`.
    pub(crate) key: String,
    pub(crate) statement: String,
    pub(crate) about: Option<String>,
    /// "semantic" | "episodic" | "consolidated".
    pub(crate) memory_class: &'static str,
    pub(crate) fact_class: Option<String>,
    /// `(origin_ref, modification_marker)`.
    pub(crate) source: Option<(String, Option<String>)>,
    pub(crate) visibility: &'static str,
    pub(crate) collection: Option<String>,
    pub(crate) entities: Vec<EntityAssertion>,
    pub(crate) links: Vec<LinkAssertion>,
    /// The rows a summary was made from.
    pub(crate) made_from: Vec<RefRow>,
    pub(crate) row: RowSeed,
}

pub(crate) fn entity_assertions(things: &[ThingInput]) -> Vec<EntityAssertion> {
    things
        .iter()
        .map(|t| EntityAssertion {
            name: t.name.clone(),
            aliases: t.other_names.clone(),
        })
        .collect()
}

pub(crate) fn link_assertions(links: &[LinkInput]) -> Vec<LinkAssertion> {
    links
        .iter()
        .map(|l| LinkAssertion {
            from: l.from.clone(),
            kind: l.kind.clone(),
            to: l.to.clone(),
            ended: l.ended,
            confidence: l.confidence_permille.map(|p| f64::from(p) / 1000.0),
        })
        .collect()
}

/// An agent's links: a stated confidence is capped, and one left out is the cap, not 1.0.
fn capped_links(mut links: Vec<LinkAssertion>) -> Vec<LinkAssertion> {
    for link in &mut links {
        let stated = link.confidence.unwrap_or(AGENT_LINK_CONFIDENCE_CAP);
        link.confidence = Some(stated.min(AGENT_LINK_CONFIDENCE_CAP));
    }
    links
}

fn has_control(text: &str) -> bool {
    text.chars().any(|c| c.is_control() && c != '\n')
}

/// A statement: `max` characters after trimming, no control character other than `\n`.
pub(crate) fn statement_ok(statement: &str, max: usize) -> bool {
    (1..=max).contains(&statement.trim().chars().count()) && !has_control(statement)
}

/// Unicode's control and format characters, zero-width ones among them, and the line and paragraph
/// separators: what a reader of an origin would not see.
pub(crate) fn is_unseen(c: char) -> bool {
    c.is_control()
        || matches!(
            u32::from(c),
            0xAD | 0x600..=0x605
                | 0x61C
                | 0x6DD
                | 0x70F
                | 0x890..=0x891
                | 0x8E2
                | 0x180E
                | 0x200B..=0x200F
                | 0x2028..=0x202E
                | 0x2060..=0x2064
                | 0x2066..=0x206F
                | 0xFEFF
                | 0xFFF9..=0xFFFB
                | 0x110BD
                | 0x110CD
                | 0x13430..=0x1343F
                | 0x1BCA0..=0x1BCA3
                | 0x1D173..=0x1D17A
                | 0xE0001
                | 0xE0020..=0xE007F
        )
}

fn thing_name_ok(name: &str) -> bool {
    (1..=200).contains(&name.trim().chars().count()) && !name.chars().any(char::is_control)
}

/// §4 step 1, the things and links.
pub(crate) fn validate_things(
    about: Option<&str>,
    things: &[ThingInput],
    links: &[LinkInput],
) -> Result<(), MemoryError> {
    let bad_things = || MemoryError::invalid("MEM_ENTITIES_INVALID", "entities");
    let bad_links = || MemoryError::invalid("MEM_LINKS_INVALID", "links");
    if things.len() > 16 {
        return Err(bad_things());
    }
    let mut names = std::collections::HashSet::new();
    for thing in things {
        if !thing_name_ok(&thing.name)
            || thing.other_names.len() > 8
            || !thing.other_names.iter().all(|n| thing_name_ok(n))
        {
            return Err(bad_things());
        }
        names.insert(memory::text::normalise_name(&thing.name));
        names.extend(
            thing
                .other_names
                .iter()
                .map(|n| memory::text::normalise_name(n)),
        );
    }
    if let Some(about) = about {
        names.insert(memory::text::normalise_name(about));
    }
    if links.len() > 16 {
        return Err(bad_links());
    }
    for link in links {
        let named = |end: &str| {
            (1..=200).contains(&end.chars().count())
                && names.contains(&memory::text::normalise_name(end))
        };
        if !named(&link.from)
            || !named(&link.to)
            || !(1..=64).contains(&link.kind.trim().chars().count())
            || link.confidence_permille.is_some_and(|p| p > 1000)
        {
            return Err(bad_links());
        }
    }
    Ok(())
}

/// §4 step 1.
pub(crate) fn validate_input(
    input: &RememberInput,
    max_statement: usize,
) -> Result<(), MemoryError> {
    if !statement_ok(&input.statement, max_statement) {
        return Err(MemoryError::invalid("MEM_STATEMENT_INVALID", "statement"));
    }
    if let Some(about) = &input.about {
        if !(1..=200).contains(&about.chars().count()) {
            return Err(MemoryError::invalid("MEM_ABOUT_INVALID", "about"));
        }
    }
    if let Some(class) = &input.fact_class {
        if !(1..=128).contains(&class.chars().count()) {
            return Err(MemoryError::invalid("MEM_CLASS_INVALID", "fact_class"));
        }
    }
    if let Some(source) = &input.source {
        validate_source(source)?;
    }
    validate_things(input.about.as_deref(), &input.entities, &input.links)?;
    let mut distinct: Vec<MemoryFactId> = input.made_from.clone();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != input.made_from.len()
        || (!input.made_from.is_empty() && !(2..=64).contains(&input.made_from.len()))
    {
        return Err(MemoryError::invalid("MEM_MADE_FROM_INVALID", "made_from"));
    }
    Ok(())
}

fn validate_source(source: &FactSource) -> Result<(), MemoryError> {
    if !(1..=2048).contains(&source.origin_ref.chars().count())
        || !(1..=256).contains(&source.version.chars().count())
        || source.origin_ref.starts_with(PAGE_ORIGIN)
        || source.origin_ref.chars().any(is_unseen)
        || source.version.chars().any(is_unseen)
    {
        return Err(MemoryError::invalid("MEM_SOURCE_INVALID", "source"));
    }
    Ok(())
}

/// §4 step 2: the agent's memory rules govern the whole write.
pub(crate) fn check_class(
    rules: &crate::model::AgentMemoryRules,
    class: Option<&str>,
) -> Result<(), MemoryError> {
    if let Some(class) = class {
        if rules.excludes.iter().any(|c| c == class) {
            return Err(MemoryError::Refused {
                code: "MEM_CLASS_EXCLUDED",
            });
        }
    }
    if rules.retains.is_empty() {
        return Ok(());
    }
    match class {
        None => Err(MemoryError::Refused {
            code: "MEM_CLASS_REQUIRED",
        }),
        Some(class) if rules.retains.iter().any(|c| c == class) => Ok(()),
        Some(_) => Err(MemoryError::Refused {
            code: "MEM_CLASS_NOT_RETAINED",
        }),
    }
}

/// §4 step 3 as the ADR-111 change gives it.
/// MEM-REP-1: the most generations of one statement tried before a write gives up looking for a free key.
const REPEAT_GENERATIONS_MAX: u32 = 64;

/// MEM-REP-1: `<hash>` for the first time a statement is kept in a place, `<hash>:g<n>` for the n-th
/// time after earlier ones were forgotten, retired, not kept or replaced.
fn generation_key(base: &str, generation: u32) -> String {
    if generation == 0 {
        base.to_owned()
    } else {
        format!("{base}:g{generation}")
    }
}

fn remember_key(input: &RememberInput) -> Result<String, MemoryError> {
    let compact = |value: Value| -> Result<String, MemoryError> {
        match &value {
            Value::Array(items) if items.is_empty() => Ok(String::new()),
            other => serde_json::to_string(other).map_err(|_| MemoryError::internal()),
        }
    };
    let entities =
        compact(serde_json::to_value(&input.entities).map_err(|_| MemoryError::internal())?)?;
    let links = compact(serde_json::to_value(&input.links).map_err(|_| MemoryError::internal())?)?;
    let mut made_from: Vec<String> = input.made_from.iter().map(ToString::to_string).collect();
    made_from.sort();
    let hashed = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        input.statement.nfc().collect::<String>(),
        input.about.as_deref().unwrap_or(""),
        input.fact_class.as_deref().unwrap_or(""),
        entities,
        links,
        made_from.join(","),
        // A document's statement is one fact per version it was read from.
        input
            .source
            .as_ref()
            .map(|s| format!("{}\n{}", s.origin_ref, s.version))
            .unwrap_or_default()
    );
    Ok(sha256_hex(&hashed))
}

impl MemoryService {
    /// Ordin Host's `remember` tool, and the MCP Front through the tool provider.
    pub async fn remember(
        &self,
        who: MemoryCaller,
        input: RememberInput,
    ) -> Result<RememberReceipt, MemoryError> {
        let agent = agent_of(&who);
        self.inner
            .remember_as(who, agent, input, &new_correlation_id())
            .await
    }
}

/// The agent or scope a write and a quota are charged to, told apart though every caller of a place
/// shares its `owner.user`; `None` for ordin, a person and the knowledge paths.
// spec: ordin-desktop/components/MemoryService#SA-LINK-01
// spec: ordin-desktop/components/MemoryService#SA-QUOTA-01
fn author_of(who: &MemoryCaller, agent: Option<AgentInstanceId>) -> Option<String> {
    match who {
        MemoryCaller::Agent { .. } | MemoryCaller::CrewMember { .. } => {
            agent.map(|a| format!("agent:{a}"))
        }
        MemoryCaller::HostedAgent { scope_id, .. } => Some(match agent {
            Some(a) => format!("agent:{a}"),
            None => format!("scope:{scope_id}"),
        }),
        _ => None,
    }
}

/// The agent whose rules bind a caller (§4 step 2).
pub(crate) fn agent_of(who: &MemoryCaller) -> Option<AgentInstanceId> {
    match who {
        MemoryCaller::CrewMember {
            agent_instance_id, ..
        }
        | MemoryCaller::Agent {
            agent_instance_id, ..
        } => Some(*agent_instance_id),
        _ => None,
    }
}

impl Inner {
    /// `agent`: the agent whose rules bind this call. The tool provider names it for a hosted agent,
    /// whose `MemoryCaller` carries none.
    pub(crate) async fn remember_as(
        &self,
        who: MemoryCaller,
        agent: Option<AgentInstanceId>,
        input: RememberInput,
        correlation_id: &str,
    ) -> Result<RememberReceipt, MemoryError> {
        self.gate()?;
        if matches!(who, MemoryCaller::KnowledgeAgent { .. }) {
            return Err(MemoryError::Refused {
                code: "MEM_TRUST_NOT_ALLOWED",
            });
        }
        validate_input(&input, 500)?;
        let rules = agent.and_then(|a| self.rules_of(a));
        if let Some(rules) = &rules {
            check_class(rules, input.fact_class.as_deref())?;
        }
        self.model_gate()?;
        // MEM-REP-1: a statement already kept (or still being kept) in this place is not kept again.
        // spec: ordin-desktop/components/MemoryService#MEM-REP-1
        let key = {
            let base = remember_key(&input)?;
            let owner = Inner::owner_of(place_of(&who));
            let (this_run, _) = run_of(&who);
            let mut chosen = None;
            for generation in 0..REPEAT_GENERATIONS_MAX {
                let key = bounded_key(generation_key(&base, generation));
                let fact = pipeline_fact_id(&owner, &key);
                match store::ref_by_fact(&self.db, &fact).await? {
                    None => {
                        chosen = Some(key);
                        break;
                    }
                    // Within one run the same call answers as before, whatever became of it (a replay
                    // after a forget does not land again, `write`); across runs a kept or pending fact
                    // is the answer.
                    Some(row)
                        if row.run_id == this_run
                            || matches!(row.state, FactState::Kept | FactState::Pending) =>
                    {
                        return Ok(RememberReceipt {
                            id: row.id,
                            state: row.state,
                        });
                    }
                    Some(_) => {}
                }
            }
            chosen.ok_or_else(MemoryError::internal)?
        };
        let author = author_of(&who, agent);
        if let Some(author) = &author {
            self.charge_quota(author)?;
        }
        let place = place_of(&who);
        let (run_id, crew_run_id) = run_of(&who);
        if !input.made_from.is_empty() && !matches!(who, MemoryCaller::Ordin { .. }) {
            return Err(MemoryError::Refused {
                code: "MEM_MADE_FROM_NOT_ALLOWED",
            });
        }
        let made_from = self.made_from_rows(place, &input.made_from).await?;
        let mut marker = None;
        if let Some(source) = &input.source {
            self.screen_source(place, source).await?;
            marker = self
                .ensure_document_source(place, &source.origin_ref, &source.version)
                .await?;
        }

        let (origin_kind, caller_kind) = origin_of(&who, input.source.is_some());
        let mut seed = RowSeed::of(origin_kind, caller_kind);
        seed.agent_instance_id = agent_of(&who);
        seed.run_id = run_id;
        seed.crew_run_id = crew_run_id;
        seed.author = author;
        // Only an autonomous write: a conversation's is the person's own words.
        if origin_kind == "agent" {
            seed.gate_trust = Some(AGENT_UNSOURCED_TRUST);
        }
        if let Some(source) = &input.source {
            seed.origin_ref = Some(source.origin_ref.clone());
            seed.origin_version = Some(source.version.clone());
        }
        seed.retain_until = rules
            .as_ref()
            .and_then(|r| r.max_age_days)
            .map(|days| Timestamp(Timestamp::now().0 + ChronoDuration::days(i64::from(days))));

        let retains = rules.as_ref().map(|r| r.retains.as_slice());
        let write = Write {
            place,
            context: self.caller_context(&who, retains),
            key,
            memory_class: if made_from.is_empty() {
                "episodic"
            } else {
                "consolidated"
            },
            source: input
                .source
                .as_ref()
                .map(|s| (s.origin_ref.clone(), marker)),
            visibility: "team-shared",
            collection: None,
            entities: entity_assertions(&input.entities),
            links: if seed.author.is_some() {
                capped_links(link_assertions(&input.links))
            } else {
                link_assertions(&input.links)
            },
            made_from,
            row: seed,
            statement: input.statement,
            about: input.about,
            fact_class: input.fact_class,
        };
        self.write(write, correlation_id).await
    }

    /// The checks a statement passes, over the origin and version a recall will show beside it.
    async fn screen_source(
        &self,
        place: MemoryPlace,
        source: &FactSource,
    ) -> Result<(), MemoryError> {
        for text in [&source.origin_ref, &source.version] {
            let found = self.check_text(place, text.clone()).await?;
            // A citation carries dates and document numbers, so a digit run that only reads like a
            // telephone number (`Review`) stands; an address or a card number (`Redact`) does not.
            if found.personal_data == memory::service::PersonalDataFinding::Redact
                || found.instruction_like
            {
                return Err(MemoryError::invalid("MEM_SOURCE_INVALID", "source"));
            }
        }
        Ok(())
    }

    /// Whether a source a team file brings passes the checks a citation passes.
    pub(crate) async fn team_source_is_clean(
        &self,
        place: MemoryPlace,
        source: &crate::model::TeamSource,
    ) -> Result<bool, MemoryError> {
        let marker = source.modification_marker.as_deref().unwrap_or("1");
        let cited = FactSource {
            origin_ref: source.origin_ref.clone(),
            version: marker.to_owned(),
        };
        if !(1..=2048).contains(&cited.origin_ref.chars().count())
            || !(1..=256).contains(&cited.version.chars().count())
            || cited.origin_ref.chars().any(is_unseen)
            || cited.version.chars().any(is_unseen)
        {
            return Ok(false);
        }
        match self.screen_source(place, &cited).await {
            Ok(()) => Ok(true),
            Err(MemoryError::Invalid { .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// A document's source row, before its first write (SA-TRUST-01). An existing one keeps its trust
    /// and its version, and this answers the version the write must carry: the pipeline writes what
    /// the job says, and a citation does not say which version of a document is the current one.
    async fn ensure_document_source(
        &self,
        place: MemoryPlace,
        origin_ref: &str,
        version: &str,
    ) -> Result<Option<String>, MemoryError> {
        let owner = Inner::owner_of(place);
        let id = source_id(&owner, origin_ref);
        let context = self.place_context(place);
        let existing = self
            .storage
            .get_source(&context, &id)
            .await
            .map_err(store_error_of)?;
        if let Some(existing) = existing {
            return Ok(existing.modification_marker);
        }
        self.storage
            .put_source(&Source {
                id,
                origin_ref: origin_ref.to_owned(),
                modification_marker: Some(version.to_owned()),
                trust_signal: DOCUMENT_TRUST,
                owner,
                collection: String::new(),
            })
            .await
            .map_err(store_error_of)?;
        Ok(Some(version.to_owned()))
    }

    /// §4 step 3 of the ADR-111 change: every id a `kept` row of the same workspace, not of a read
    /// folder.
    async fn made_from_rows(
        &self,
        place: MemoryPlace,
        ids: &[MemoryFactId],
    ) -> Result<Vec<RefRow>, MemoryError> {
        let rows = store::refs_by_ids(&self.db, ids).await?;
        let key = store::scope_key(place);
        let good = rows.len() == ids.len()
            && rows.iter().all(|row| {
                row.state == FactState::Kept && row.scope_key == key && row.read_folder_id.is_none()
            });
        if good {
            // In the order the caller gave.
            let mut ordered = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(row) = rows.iter().find(|r| r.id == *id) {
                    ordered.push(row.clone());
                }
            }
            Ok(ordered)
        } else {
            Err(MemoryError::invalid("MEM_MADE_FROM_INVALID", "made_from"))
        }
    }

    /// §4 steps 3 to 7.
    pub(crate) async fn write(
        &self,
        write: Write,
        correlation_id: &str,
    ) -> Result<RememberReceipt, MemoryError> {
        let Write {
            place,
            mut context,
            key,
            statement,
            about,
            memory_class,
            fact_class,
            source,
            visibility,
            collection,
            entities,
            links,
            made_from,
            row: seed,
        } = write;
        let key = bounded_key(key);
        let owner = Inner::owner_of(place);
        let memory_fact_id = pipeline_fact_id(&owner, &key);
        // A forget cancels the job and so the dedupe it gave: the same write must not land again.
        if let Some(row) = store::ref_by_fact(&self.db, &memory_fact_id).await? {
            if row.state == FactState::Forgotten {
                return Ok(RememberReceipt {
                    id: row.id,
                    state: row.state,
                });
            }
        }
        let mut content = json!({ "statement": statement });
        if let Some(about) = &about {
            content["subject"] = Value::String(about.clone());
        }
        let mut body = json!({
            "content": content,
            "memory_class": memory_class,
            "visibility": visibility,
        });
        if let Some(class) = &fact_class {
            body["fact_class"] = Value::String(class.clone());
        }
        if let Some((origin_ref, marker)) = &source {
            body["source"] = json!({ "origin_ref": origin_ref, "modification_marker": marker });
        }
        if let Some(collection) = &collection {
            body["collection"] = Value::String(collection.clone());
        }
        if !entities.is_empty() {
            body["entities"] =
                serde_json::to_value(&entities).map_err(|_| MemoryError::internal())?;
        }
        if !links.is_empty() {
            body["links"] = serde_json::to_value(&links).map_err(|_| MemoryError::internal())?;
        }
        if !made_from.is_empty() {
            let origins: Vec<&str> = made_from
                .iter()
                .map(|r| r.memory_fact_id.as_str())
                .collect();
            body["derived_from"] = json!(origins);
        }
        if let Some(trust) = seed.gate_trust {
            body["gate_trust"] = json!(trust);
        }
        if let Some(author) = &seed.author {
            body["author"] = json!(author);
        }
        let body = serde_json::to_vec(&body).map_err(|_| MemoryError::internal())?;

        context.correlation_id = correlation_id.to_owned();
        let guard = self.auth.mint(context).await?;
        let ack = self
            .service
            .remember(guard.cx(correlation_id, Some(&key)), &body)
            .await
            .map_err(map_remember_error)?
            .data;
        drop(guard);

        // Step 6.
        if let Some(row) = store::ref_by_fact(&self.db, &memory_fact_id).await? {
            return Ok(RememberReceipt {
                id: row.id,
                state: row.state,
            });
        }
        let replay = matches!(ack.status, JobAckStatus::AlreadyAccepted);
        let row = new_row(place, memory_fact_id, &ack.job_id, fact_class, seed);
        if let Err(error) = store::insert_ref(&self.db, &row).await {
            // Two calls of one fact at once: the first row stands.
            if let Some(row) = store::ref_by_fact(&self.db, &row.memory_fact_id).await? {
                return Ok(RememberReceipt {
                    id: row.id,
                    state: row.state,
                });
            }
            return Err(error);
        }
        if !made_from.is_empty() {
            let origins: Vec<MemoryFactId> = made_from.iter().map(|r| r.id).collect();
            store::insert_made_from(&self.db, row.id, &origins).await?;
        }

        // Step 7. A replay wrote nothing new.
        if !replay {
            self.record_written(
                &row,
                entities.len(),
                links.len(),
                made_from.len(),
                correlation_id,
            )
            .await;
        }
        Ok(RememberReceipt {
            id: row.id,
            state: FactState::Pending,
        })
    }

    async fn record_written(
        &self,
        row: &RefRow,
        things: usize,
        links: usize,
        made_from: usize,
        correlation_id: &str,
    ) {
        let fact_ref = row.id.to_string();
        if made_from > 0 {
            self.journal(
                "fact_consolidated",
                row.run_id,
                None,
                row.workspace_id,
                "A summary was written.",
                json!({ "fact_ref": fact_ref, "made_from": made_from }),
            )
            .await;
            tracing::info!(
                target: "ordin::memory",
                fact_ref = %row.id,
                made_from,
                correlation_id,
                "memory.fact_consolidated"
            );
            return;
        }
        self.journal(
            "fact_written",
            row.run_id,
            None,
            row.workspace_id,
            "A fact was written.",
            json!({
                "fact_ref": fact_ref,
                "scope_key": row.scope_key,
                "origin_kind": row.origin_kind,
                "fact_class": row.fact_class,
            }),
        )
        .await;
        tracing::info!(
            target: "ordin::memory",
            fact_ref = %row.id,
            scope_key = %row.scope_key,
            origin_kind = %row.origin_kind,
            fact_class = row.fact_class.as_deref().unwrap_or(""),
            things,
            links,
            correlation_id,
            "memory.fact_written"
        );
    }

    /// The fact of a row, read under its place's context.
    pub(crate) async fn fact_of(&self, row: &RefRow) -> Result<Option<Fact>, MemoryError> {
        let place = row.place()?;
        let guard = self.auth.mint(self.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        match self
            .service
            .get_fact(guard.cx(&correlation_id, None), &row.memory_fact_id)
            .await
        {
            Ok(result) => Ok(Some(result.data)),
            Err(error) if crate::error::is_not_found(&error) => Ok(None),
            Err(error) => Err(crate::error::map_call_error(error)),
        }
    }

    /// `Service::delete` under a place's context, keeping the component's error for the caller.
    pub(crate) async fn delete_fact(
        &self,
        place: MemoryPlace,
        fact: &str,
        key: &str,
    ) -> Result<memory::types::api::DeletionProof, CallError> {
        let guard = self
            .auth
            .mint(self.place_context(place))
            .await
            .map_err(|_| CallError::from(AppError::Internal))?;
        let correlation_id = new_correlation_id();
        let key = bounded_key(key.to_owned());
        self.service
            .delete(guard.cx(&correlation_id, Some(&key)), fact)
            .await
            .map(|result| result.data)
    }

    /// `Service::retire` under a place's context; true when a fact was retired, false when none is
    /// stored yet.
    pub(crate) async fn retire_fact(
        &self,
        place: MemoryPlace,
        fact: &str,
        key: &str,
    ) -> Result<bool, MemoryError> {
        let guard = self.auth.mint(self.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let key = bounded_key(key.to_owned());
        match self
            .service
            .retire(guard.cx(&correlation_id, Some(&key)), fact)
            .await
        {
            Ok(_) => Ok(true),
            Err(error) if crate::error::is_not_found(&error) => Ok(false),
            Err(error) => Err(crate::error::map_call_error(error)),
        }
    }

    /// §9 steps 4 and 5: the new fact is kept, so the old one closes.
    // §6 step 3 and §9 say `Service::retire`, which only ends the validity. The scenarios
    // of ADR-103 and ADR-111 need more: an export carries the old fact's `superseded_by`, and
    // `fact_history` walks `supersedes` to list the versions. The component has no `Service`
    // operation that records them, so this calls `MemoryStore::supersede` directly, under the place's
    // context, which also closes what `retire` closes.
    pub(crate) async fn supersede_fact(
        &self,
        old: &RefRow,
        new_fact: &str,
    ) -> Result<(), MemoryError> {
        let context = self.place_context(old.place()?);
        match self
            .storage
            .supersede(&context, &old.memory_fact_id, new_fact, chrono::Utc::now())
            .await
        {
            Ok(()) | Err(memory::types::ports::StoreError::NotFound) => Ok(()),
            Err(error) => Err(store_error_of(error)),
        }
    }
}

/// The run a caller acts in.
fn run_of(who: &MemoryCaller) -> (Option<RunId>, Option<CrewRunId>) {
    match who {
        MemoryCaller::Ordin { run_id, .. }
        | MemoryCaller::Agent { run_id, .. }
        | MemoryCaller::HostedAgent { run_id, .. } => (Some(*run_id), None),
        MemoryCaller::CrewMember {
            run_id,
            crew_run_id,
            ..
        } => (Some(*run_id), Some(*crew_run_id)),
        MemoryCaller::Person { .. }
        | MemoryCaller::KnowledgeService { .. }
        | MemoryCaller::KnowledgeAgent { .. } => (None, None),
    }
}

/// §4 step 6: the row's `origin_kind` and `caller_kind`.
fn origin_of(who: &MemoryCaller, has_source: bool) -> (&'static str, &'static str) {
    let (origin, caller) = match who {
        MemoryCaller::Ordin { .. } => ("conversation", "ordin"),
        MemoryCaller::CrewMember { .. } => ("agent", "crew_member"),
        MemoryCaller::Agent { .. } => ("agent", "agent"),
        MemoryCaller::HostedAgent { .. } => ("conversation", "hosted_agent"),
        MemoryCaller::Person { .. } => ("person", "person"),
        MemoryCaller::KnowledgeService { .. } => ("knowledge_page", "knowledge_service"),
        MemoryCaller::KnowledgeAgent { .. } => ("document", "hosted_agent"),
    };
    if has_source {
        ("document", caller)
    } else {
        (origin, caller)
    }
}

fn new_row(
    place: MemoryPlace,
    memory_fact_id: String,
    job_id: &str,
    fact_class: Option<String>,
    seed: RowSeed,
) -> RefRow {
    let (workspace_id, scope_id) = match place {
        MemoryPlace::Workspace(w) => (Some(w), None),
        MemoryPlace::Scope(s) => (None, Some(s)),
    };
    RefRow {
        id: MemoryFactId::new(),
        memory_fact_id,
        scope_key: store::scope_key(place),
        workspace_id,
        scope_id,
        everywhere: seed.everywhere,
        origin_kind: seed.origin_kind.to_owned(),
        agent_instance_id: seed.agent_instance_id,
        run_id: seed.run_id,
        origin_ref: seed.origin_ref,
        origin_version: seed.origin_version,
        fact_class,
        job_id: job_id.to_owned(),
        state: FactState::Pending,
        may_be_out_of_date: seed.may_be_out_of_date,
        replaces_id: seed.replaces_id,
        retain_until: seed.retain_until,
        written_at: Timestamp::now(),
        settled_at: None,
        shared_by: None,
        shared_at: None,
        first_exported_at: None,
        team_imported: false,
        read_folder_id: seed.read_folder_id,
        caller_kind: Some(seed.caller_kind.to_owned()),
        page_status: seed.page_status,
        page_tier: seed.page_tier,
    }
}
