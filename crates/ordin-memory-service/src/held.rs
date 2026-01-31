//! The writes the gate holds for a person (SA-TRUST-04), and what one agent may write (SA-QUOTA-01).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use memory::types::domain::{MemoryClass, Source};
use memory::write_pipeline::QuarantineRecord;
use ordin_types::time::Timestamp;
use ordin_types::{MemoryFactId, WorkspaceId};
use serde_json::{json, Value};

use crate::config::TENANT;
use crate::error::MemoryError;
use crate::model::{FactState, HeldLink, HeldThing, HeldWrite, MemoryPlace, RememberReceipt};
use crate::remember::{sha256_hex, RowSeed, Write};
use crate::service::{new_correlation_id, store_error_of, Inner, MemoryService};
use crate::store;

/// Held writes one review shows, newest first.
const HELD_PAGE: u32 = 200;
/// The trust a person's keep gives a held write: the person is the source now.
const KEPT_TRUST: f64 = 1.0;
/// The author of what a person keeps from a team file: it writes new links and changes no others.
const TEAM_AUTHOR: &str = "team";

/// Writes one agent makes in a minute.
const PER_MINUTE: u32 = 60;
/// Writes one agent makes in a day.
const PER_DAY: u32 = 2_000;
const MINUTE: Duration = Duration::from_secs(60);
const DAY: Duration = Duration::from_secs(86_400);
/// Above this many agents, those past both windows are forgotten.
const TRACKED: usize = 1_024;

#[derive(Debug, Clone, Copy)]
struct Window {
    started: Instant,
    used: u32,
}

impl Window {
    fn new(now: Instant) -> Window {
        Window {
            started: now,
            used: 0,
        }
    }

    fn roll(&mut self, now: Instant, length: Duration) {
        if now.duration_since(self.started) >= length {
            *self = Window::new(now);
        }
    }
}

/// Fixed windows per agent, in memory: a restart gives an agent a new window, and an agent cannot
/// restart the program.
// spec: ordin-desktop/components/MemoryService#SA-QUOTA-01
#[derive(Debug, Default)]
pub(crate) struct WriteQuotas {
    agents: HashMap<String, (Window, Window)>,
}

impl WriteQuotas {
    /// Takes one write from `author`'s windows, or says none is left. A refused write takes nothing.
    pub(crate) fn charge(&mut self, author: &str, now: Instant) -> bool {
        if self.agents.len() >= TRACKED {
            self.agents
                .retain(|_, (_, day)| now.duration_since(day.started) < DAY);
        }
        let (minute, day) = self
            .agents
            .entry(author.to_owned())
            .or_insert_with(|| (Window::new(now), Window::new(now)));
        minute.roll(now, MINUTE);
        day.roll(now, DAY);
        if minute.used >= PER_MINUTE || day.used >= PER_DAY {
            return false;
        }
        minute.used += 1;
        day.used += 1;
        true
    }
}

impl Inner {
    pub(crate) fn charge_quota(&self, author: &str) -> Result<(), MemoryError> {
        let charged = self
            .quotas
            .lock()
            .map_err(|_| MemoryError::internal())?
            .charge(author, Instant::now());
        if charged {
            return Ok(());
        }
        tracing::info!(target: "ordin::memory", "memory.write_quota_reached");
        Err(MemoryError::Unavailable {
            code: "MEM_RATE_LIMITED",
            retryable: true,
        })
    }

    async fn held_record(
        &self,
        place: MemoryPlace,
        id: &str,
    ) -> Result<QuarantineRecord, MemoryError> {
        let team = Inner::owner_of(place).team;
        self.storage
            .held_write(TENANT, team.as_deref(), id)
            .await
            .map_err(store_error_of)?
            .ok_or(MemoryError::Refused {
                code: "MEM_NOT_HELD",
            })
    }

    async fn row_of_held(
        &self,
        record: &QuarantineRecord,
    ) -> Result<Option<store::RefRow>, MemoryError> {
        match &record.fact_id {
            Some(fact) => store::ref_by_fact(&self.db, fact).await,
            None => Ok(None),
        }
    }

    /// The held write's row settles now rather than at §6's ten minutes, so a run's list does not show
    /// it beside the kept one.
    async fn close_held_row(&self, row: Option<&store::RefRow>) -> Result<(), MemoryError> {
        if let Some(row) = row {
            let (from, to) = (FactState::Pending, FactState::NotKept);
            store::settle(&self.db, row.id, from, to, Timestamp::now()).await?;
        }
        Ok(())
    }

    async fn cited_by(
        &self,
        place: MemoryPlace,
        record: &QuarantineRecord,
    ) -> Result<Option<memory::types::domain::Source>, MemoryError> {
        let Some(id) = &record.source_id else {
            return Ok(None);
        };
        self.storage
            .get_source(&self.place_context(place), id)
            .await
            .map_err(store_error_of)
    }
}

fn text_of(content: &Value, field: &str) -> Option<String> {
    content
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// What a keep would write, bound so that a file re-imported under the same id cannot change it
/// after the person read it.
// spec: ordin-desktop/components/MemoryService#SA-TRUST-05
fn content_hash(record: &QuarantineRecord, cited: Option<&Source>) -> String {
    sha256_hex(
        &json!({
            "fact_id": record.fact_id,
            "content": record.content,
            "memory_class": format!("{:?}", record.memory_class),
            "fact_class": record.fact_class,
            "collection": record.collection,
            "entities": record.entities_asserted,
            "links": record.links_asserted,
            "cited": cited.map(|s| (&s.origin_ref, &s.modification_marker)),
        })
        .to_string(),
    )
}

fn permille(score: f64) -> u16 {
    (score.clamp(0.0, 1.0) * 1000.0).round() as u16
}

/// A row's `origin_kind` and `caller_kind` as the new row carries them.
fn origin_of_row(row: Option<&store::RefRow>) -> (&'static str, &'static str) {
    let Some(row) = row else {
        return ("person", "person");
    };
    let caller = match row.caller_kind.as_deref() {
        Some("crew_member") => "crew_member",
        Some("agent") => "agent",
        Some("hosted_agent") => "hosted_agent",
        Some("ordin") => "ordin",
        _ => "person",
    };
    match row.origin_kind.as_str() {
        "agent" => ("agent", caller),
        "document" => ("document", caller),
        "conversation" => ("conversation", caller),
        _ => ("person", caller),
    }
}

impl MemoryService {
    /// The writes held for a person of `place`, newest first, at most 200.
    // spec: ordin-desktop/components/MemoryService#SA-TRUST-04
    pub async fn held_writes(&self, place: MemoryPlace) -> Result<Vec<HeldWrite>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let team = Inner::owner_of(place).team;
        let records = inner
            .storage
            .held_writes(TENANT, team.as_deref(), HELD_PAGE)
            .await
            .map_err(store_error_of)?;
        let mut out = Vec::with_capacity(records.len());
        for record in records {
            let row = inner.row_of_held(&record).await?;
            // A held write of a forgotten fact goes with it; this clears one from before that did.
            if row
                .as_ref()
                .is_some_and(|r| r.state == FactState::Forgotten)
            {
                inner
                    .storage
                    .drop_quarantine(&record)
                    .await
                    .map_err(store_error_of)?;
                continue;
            }
            let cited = inner.cited_by(place, &record).await?;
            out.push(HeldWrite {
                statement: text_of(&record.content, "statement")
                    .unwrap_or_else(|| record.content.to_string()),
                about: text_of(&record.content, "subject"),
                fact_class: record.fact_class.clone(),
                content_hash: content_hash(&record, cited.as_ref()),
                cited: cited.map(|s| s.origin_ref),
                things: record
                    .entities_asserted
                    .iter()
                    .map(|e| HeldThing {
                        name: e.name.clone(),
                        other_names: e.aliases.clone(),
                    })
                    .collect(),
                links: record
                    .links_asserted
                    .iter()
                    .map(|l| HeldLink {
                        from: l.from.clone(),
                        kind: l.kind.clone(),
                        to: l.to.clone(),
                        ended: l.ended,
                        confidence_permille: l.confidence.map(permille),
                    })
                    .collect(),
                trust_permille: permille(record.trust_score),
                written_by: row.and_then(|r| r.agent_instance_id),
                held_at: Timestamp(record.quarantined_at),
                needs_check: record.pii_review,
                id: record.id,
            });
        }
        Ok(out)
    }

    /// The person keeps a held write: it runs the write path again, screens included, with the
    /// person's trust. `content_hash` is the one `held_writes` showed. `MEM_NOT_HELD` when no such
    /// write waits, it has no fact to keep, or its fact was forgotten; `MEM_HELD_CHANGED` when it is
    /// not what the person saw.
    pub async fn keep_held(
        &self,
        place: MemoryPlace,
        id: &str,
        seen_hash: &str,
    ) -> Result<RememberReceipt, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.model_gate()?;
        let record = inner.held_record(place, id).await?;
        // A row written before `fact_id` has no origin to carry and no forget to check, so a person
        // can discard it and cannot keep it.
        if record.fact_id.is_none() {
            return Err(MemoryError::Refused {
                code: "MEM_NOT_HELD",
            });
        }
        let row = inner.row_of_held(&record).await?;
        if row
            .as_ref()
            .is_some_and(|r| r.state == FactState::Forgotten)
        {
            inner
                .storage
                .drop_quarantine(&record)
                .await
                .map_err(store_error_of)?;
            return Err(MemoryError::Refused {
                code: "MEM_NOT_HELD",
            });
        }
        let statement = text_of(&record.content, "statement").ok_or(MemoryError::Refused {
            code: "MEM_NOT_HELD",
        })?;
        let source = inner.cited_by(place, &record).await?;
        if content_hash(&record, source.as_ref()) != seen_hash {
            return Err(MemoryError::Refused {
                code: "MEM_HELD_CHANGED",
            });
        }
        let (origin_kind, caller_kind) = origin_of_row(row.as_ref());
        let mut seed = RowSeed::of(origin_kind, caller_kind);
        seed.gate_trust = Some(KEPT_TRUST);
        seed.author = Some(TEAM_AUTHOR.to_owned());
        if let Some(row) = &row {
            seed.agent_instance_id = row.agent_instance_id;
            seed.run_id = row.run_id;
            seed.origin_ref.clone_from(&row.origin_ref);
            seed.origin_version.clone_from(&row.origin_version);
            seed.retain_until = row.retain_until;
            seed.author = match (row.agent_instance_id, row.team_imported) {
                (Some(agent), _) => Some(format!("agent:{agent}")),
                (None, true) => Some(TEAM_AUTHOR.to_owned()),
                (None, false) => None,
            };
        }
        let correlation_id = new_correlation_id();
        let write = Write {
            place,
            context: inner.place_context(place),
            key: format!("held:{}", record.id),
            about: text_of(&record.content, "subject"),
            statement,
            // A held summary has no origins to name here, so it is kept as what it states.
            memory_class: match record.memory_class {
                MemoryClass::Episodic => "episodic",
                MemoryClass::Semantic | MemoryClass::Consolidated => "semantic",
            },
            fact_class: record.fact_class.clone(),
            source: source.map(|s| (s.origin_ref, s.modification_marker)),
            visibility: "team-shared",
            collection: (!record.collection.is_empty()).then(|| record.collection.clone()),
            entities: record.entities_asserted.clone(),
            links: record.links_asserted.clone(),
            made_from: Vec::new(),
            row: seed,
        };
        let receipt = inner.write(write, &correlation_id).await?;
        if origin_kind == "document" && row.as_ref().is_some_and(|r| r.agent_instance_id.is_some())
        {
            store::confirm_citation(&inner.db, receipt.id, Timestamp::now()).await?;
        }
        inner
            .storage
            .drop_quarantine(&record)
            .await
            .map_err(store_error_of)?;
        inner.close_held_row(row.as_ref()).await?;
        inner
            .journal(
                "held_kept",
                None,
                None,
                row.and_then(|r| r.workspace_id),
                "The person kept a held fact.",
                json!({ "fact_ref": receipt.id.to_string() }),
            )
            .await;
        tracing::info!(target: "ordin::memory", fact_ref = %receipt.id, "memory.held_kept");
        Ok(receipt)
    }

    /// The kept facts whose agent cited a source no person has confirmed. They stay out of the team
    /// folder until `confirm_citation`.
    // spec: ordin-desktop/components/MemoryService#SA-CITE-02
    pub async fn citations_to_confirm(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<MemoryFactId>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        store::unconfirmed_citations(
            &inner.db,
            &store::scope_key(MemoryPlace::Workspace(workspace_id)),
        )
        .await
    }

    /// The person stands behind an agent's citation. `MEM_NOT_HELD` for anything else.
    pub async fn confirm_citation(
        &self,
        workspace_id: WorkspaceId,
        id: MemoryFactId,
    ) -> Result<(), MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let scope_key = store::scope_key(MemoryPlace::Workspace(workspace_id));
        if !store::unconfirmed_citations(&inner.db, &scope_key)
            .await?
            .contains(&id)
        {
            return Err(MemoryError::Refused {
                code: "MEM_NOT_HELD",
            });
        }
        store::confirm_citation(&inner.db, id, Timestamp::now()).await?;
        tracing::info!(target: "ordin::memory", fact_ref = %id, "memory.citation_confirmed");
        Ok(())
    }

    /// The person throws a held write away. Nothing was stored but the held row.
    pub async fn discard_held(&self, place: MemoryPlace, id: &str) -> Result<(), MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let record = inner.held_record(place, id).await?;
        let row = inner.row_of_held(&record).await?;
        inner
            .storage
            .drop_quarantine(&record)
            .await
            .map_err(store_error_of)?;
        inner.close_held_row(row.as_ref()).await?;
        let fact_ref: Option<MemoryFactId> = row.as_ref().map(|r| r.id);
        inner
            .journal(
                "held_discarded",
                None,
                None,
                row.and_then(|r| r.workspace_id),
                "The person threw a held fact away.",
                json!({ "fact_ref": fact_ref.map(|id| id.to_string()) }),
            )
            .await;
        tracing::info!(target: "ordin::memory", "memory.held_discarded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_has_60_writes_a_minute_and_2000_a_day() {
        let mut quotas = WriteQuotas::default();
        let start = Instant::now();
        for _ in 0..PER_MINUTE {
            assert!(quotas.charge("agent:a", start));
        }
        assert!(!quotas.charge("agent:a", start));
        assert!(quotas.charge("agent:b", start), "each agent has its own");
        let mut now = start;
        let mut taken = PER_MINUTE;
        while taken < PER_DAY {
            now += MINUTE;
            for _ in 0..PER_MINUTE.min(PER_DAY - taken) {
                assert!(quotas.charge("agent:a", now));
                taken += 1;
            }
        }
        now += MINUTE;
        assert!(!quotas.charge("agent:a", now), "the day's writes are spent");
        assert!(quotas.charge("agent:a", start + DAY + MINUTE));
    }

    #[test]
    fn a_refused_write_takes_nothing_from_the_day() {
        let mut quotas = WriteQuotas::default();
        let start = Instant::now();
        for _ in 0..PER_MINUTE * 3 {
            quotas.charge("agent:a", start);
        }
        let (_, day) = quotas.agents["agent:a"];
        assert_eq!(day.used, PER_MINUTE);
    }

    #[test]
    fn agents_past_their_day_are_forgotten() {
        let mut quotas = WriteQuotas::default();
        let start = Instant::now();
        for n in 0..TRACKED {
            quotas.charge(&format!("agent:{n}"), start);
        }
        quotas.charge("agent:new", start + DAY);
        assert_eq!(quotas.agents.len(), 1);
    }
}
