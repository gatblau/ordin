//! The mappings every result shares: rule FV (`FactView`, with rule J) and rule LV (`LinkView`).

use std::collections::HashMap;

use chrono::Utc;
use memory::maintenance::{fading_since, is_fading};
use memory::types::domain::{Fact, MemoryClass, Relationship};
use memory::types::scope::ScopeContext;
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, MemoryFactId, MemoryLinkId, MemoryThingId};

use crate::db::q;
use crate::error::MemoryError;
use crate::model::{FactOrigin, FactView, LinkBy, LinkView, MemoryKind, ThingRef};
use crate::remember::PAGE_ORIGIN;
use crate::service::{store_error_of, Inner};
use crate::store::{self, RefRow};
use uuid::Uuid;

/// Names already read in this operation (rule FV: "where the operation already holds them").
pub(crate) type Names = HashMap<String, Option<String>>;

/// What one fact's view takes beyond the fact and its row.
#[derive(Default, Clone, Copy)]
pub(crate) struct ViewNote {
    /// `RankedFact.fading`, in recall.
    pub(crate) fading: Option<bool>,
    pub(crate) relevance_permille: Option<u16>,
}

fn kind_of(class: MemoryClass) -> MemoryKind {
    match class {
        MemoryClass::Episodic => MemoryKind::Happened,
        MemoryClass::Semantic => MemoryKind::Known,
        MemoryClass::Consolidated => MemoryKind::Summary,
    }
}

pub(crate) fn text_of(fact: &Fact, key: &str) -> Option<String> {
    fact.content
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// `fact:<uuid>` to the `MemoryFactId` its row would carry when it has none.
fn id_of_fact(fact: &str) -> MemoryFactId {
    MemoryFactId(
        fact.strip_prefix("fact:")
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .unwrap_or_else(Uuid::nil),
    )
}

/// Rule J and the row's origin.
fn origin_of(row: &RefRow) -> FactOrigin {
    let document = || match (&row.origin_ref, &row.origin_version) {
        (Some(origin_ref), Some(version)) => Some(FactOrigin::Document {
            origin_ref: origin_ref.clone(),
            version: version.clone(),
        }),
        _ => None,
    };
    // spec: ordin-desktop/components/MemoryService#SA-TEAM-02
    if row.team_imported {
        return document()
            .filter(|_| row.origin_kind == "document")
            .unwrap_or(FactOrigin::Team);
    }
    match row.origin_kind.as_str() {
        "conversation" => {
            row.run_id
                .map_or(FactOrigin::Person, |run_id| FactOrigin::Conversation {
                    run_id,
                })
        }
        "agent" => match (row.agent_instance_id, row.run_id) {
            (Some(agent_instance_id), Some(run_id)) => FactOrigin::Agent {
                agent_instance_id,
                run_id,
            },
            _ => FactOrigin::Person,
        },
        // spec: ordin-desktop/components/MemoryService#SA-CITE-01
        "document" => match (row.agent_instance_id, document()) {
            (
                Some(agent_instance_id),
                Some(FactOrigin::Document {
                    origin_ref,
                    version,
                }),
            ) => FactOrigin::AgentCiting {
                agent_instance_id,
                run_id: row.run_id,
                origin_ref,
                version,
            },
            (_, document) => document.unwrap_or(FactOrigin::Person),
        },
        "knowledge_page" => {
            row.page_id()
                .map_or(FactOrigin::Person, |page_id| FactOrigin::KnowledgePage {
                    page_id,
                })
        }
        _ => FactOrigin::Person,
    }
}

impl Inner {
    /// A thing's canonical name; `None` when the read filter hides it.
    // Rule FV reads an unknown name through "one `Service::entity_detail` per entity with
    // limit 0". The as-built operation has no limit, reads the entity's facts and links, and takes a
    // read token, so a recall of ten facts would spend tens of tokens of the 600 a minute. The name
    // is read by `MemoryStore::get_entity`, under the same context and the same read filter.
    async fn name_of(
        &self,
        id: &str,
        context: &ScopeContext,
        names: &mut Names,
    ) -> Result<Option<String>, MemoryError> {
        if let Some(name) = names.get(id) {
            return Ok(name.clone());
        }
        let name = self
            .storage
            .get_entity(context, id)
            .await
            .map_err(store_error_of)?
            .map(|e| e.canonical_name);
        names.insert(id.to_owned(), name.clone());
        Ok(name)
    }

    /// Rule SA-MPD-03: one `SELECT` per call, not per fact.
    pub(crate) async fn origin_rows(
        &self,
        facts: &[&Fact],
    ) -> Result<HashMap<String, RefRow>, MemoryError> {
        let ids: Vec<String> = facts
            .iter()
            .filter(|f| f.memory_class == MemoryClass::Consolidated)
            .flat_map(|f| f.derived_from.iter().cloned())
            .collect();
        Ok(store::refs_by_facts(&self.db, &ids)
            .await?
            .into_iter()
            .map(|row| (row.memory_fact_id.clone(), row))
            .collect())
    }

    /// Rule FV. `row` is None only for a version with no row (rule M).
    pub(crate) async fn fact_view(
        &self,
        fact: &Fact,
        row: Option<&RefRow>,
        note: ViewNote,
        context: &ScopeContext,
        names: &mut Names,
        origins: &HashMap<String, RefRow>,
    ) -> Result<FactView, MemoryError> {
        let now = Utc::now();
        let fading = note
            .fading
            .unwrap_or_else(|| is_fading(fact, now, &self.decay));
        let mut things = Vec::new();
        for id in fact.entities.iter().take(16) {
            if let (Some(name), Ok(id)) = (
                self.name_of(id, context, names).await?,
                MemoryThingId::parse(id),
            ) {
                things.push(ThingRef { id, name });
            }
        }
        let summary = fact.memory_class == MemoryClass::Consolidated;
        let (derived_from, derived_origins) = if summary {
            (
                fact.derived_from.iter().map(|id| id_of_fact(id)).collect(),
                fact.derived_from
                    .iter()
                    .filter_map(|id| origins.get(id))
                    .map(derived_origin)
                    .collect(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let about = text_of(fact, "subject");
        let page_id = row.and_then(RefRow::page_id);
        let page_status = row
            .and_then(|r| r.page_status)
            .filter(|s| *s != crate::model::PageStatus::Stable);
        let mut view = FactView {
            id: row.map_or_else(|| id_of_fact(&fact.id), |r| r.id),
            statement: text_of(fact, "statement").unwrap_or_default(),
            page_section: page_id
                .and(about.as_ref())
                .filter(|s| !s.is_empty())
                .cloned(),
            about,
            workspace_id: row.filter(|r| !r.everywhere).and_then(|r| r.workspace_id),
            came_from: row.map_or(FactOrigin::Person, origin_of),
            written_at: row.map_or(Timestamp(fact.ingested_at), |r| r.written_at),
            may_be_out_of_date: row.is_some_and(|r| r.may_be_out_of_date),
            page_id,
            relevance_permille: note.relevance_permille,
            shared_by: row.and_then(|r| r.shared_by.clone()),
            shared_at: row.and_then(|r| r.shared_at.or(r.first_exported_at)),
            needs_check: fact.pii_review,
            folder: None,
            folder_description: None,
            set_by_company: false,
            adopted_from: None,
            adoption_mark: None,
            kind: kind_of(fact.memory_class),
            true_from: Timestamp(fact.valid_from),
            true_until: fact.valid_to.map(Timestamp),
            replaces: row.and_then(|r| r.replaces_id),
            fading,
            not_used_since: fading.then(|| Timestamp(fading_since(fact))),
            things,
            made_from_count: summary.then_some(fact.derived_from.len() as u16),
            page_tier: row.and_then(|r| r.page_tier),
            page_status,
            derived_from,
            derived_origins,
        };
        if let Some(row) = row {
            self.label(&mut view, row).await;
        }
        Ok(view)
    }

    /// §5 (ADR-110) step 4: the folder and adoption labels.
    async fn label(&self, view: &mut FactView, row: &RefRow) {
        let (Some(folders), Some(workspace_id)) = (self.folders(), row.workspace_id) else {
            return;
        };
        let item_path = match row.page_id() {
            Some(page) => format!("pages/{page}.md"),
            None => format!("facts/{}.json", row.id),
        };
        if let Some(label) = folders
            .label_for(workspace_id, row.read_folder_id, &item_path)
            .await
        {
            view.folder = Some(label.name);
            view.folder_description = label.description;
            view.set_by_company = label.set_by_company;
        }
        if let Some(adoption) = folders
            .adoption_for(workspace_id, &row.id.to_string())
            .await
        {
            view.adopted_from = Some(adoption.adopted_from);
            view.adoption_mark = adoption.mark;
        }
    }

    /// One fact through FV under its place, for the operations that hold a single fact.
    pub(crate) async fn view_of_row(
        &self,
        fact: &Fact,
        row: &RefRow,
    ) -> Result<FactView, MemoryError> {
        let context = self.place_context(row.place()?);
        let origins = self.origin_rows(&[fact]).await?;
        self.fact_view(
            fact,
            Some(row),
            ViewNote::default(),
            &context,
            &mut Names::new(),
            &origins,
        )
        .await
    }

    // spec: ordin-desktop/components/MemoryService#SA-AMEM-06
    /// SA-AMEM-06: who made a link. One parameterised `SELECT` by relationship id.
    async fn link_by(&self, link: &Relationship) -> Result<LinkBy, MemoryError> {
        let row = self
            .db
            .first(
                q(
                    "SELECT r.origin_kind, r.agent_instance_id, r.team_imported, r.origin_ref \
                   FROM mem_relationship_fact f \
                   JOIN memory_fact_refs r ON r.memory_fact_id = f.fact_id \
                   WHERE f.relationship_id = $1 AND f.role = 'asserted' \
                   ORDER BY f.at LIMIT 1",
                )
                .text(link.id.clone()),
            )
            .await?;
        let Some(row) = row else {
            return Ok(if link.replaces.is_some() {
                LinkBy::Person
            } else {
                LinkBy::Team
            });
        };
        if row.boolean("team_imported")? {
            return Ok(LinkBy::Team);
        }
        let agent = row.opt_uuid("agent_instance_id")?;
        Ok(match (row.text("origin_kind")?.as_str(), agent) {
            ("knowledge_page", _) => row
                .opt_text("origin_ref")?
                .as_deref()
                .and_then(page_of_origin)
                .map_or(LinkBy::Team, |page_id| LinkBy::Page { page_id }),
            ("person", _) => LinkBy::Person,
            (_, Some(agent)) => LinkBy::Agent {
                agent_instance_id: ordin_types::AgentInstanceId(agent),
            },
            _ => LinkBy::Ordin,
        })
    }

    /// Rule LV. `names`: the endpoint names the component returned, when it did.
    pub(crate) async fn link_view(
        &self,
        link: &Relationship,
        from_name: Option<String>,
        to_name: Option<String>,
        context: &ScopeContext,
        names: &mut Names,
    ) -> Result<LinkView, MemoryError> {
        let from_name = match from_name {
            Some(name) => name,
            None => self
                .name_of(&link.from, context, names)
                .await?
                .unwrap_or_default(),
        };
        let to_name = match to_name {
            Some(name) => name,
            None => self
                .name_of(&link.to, context, names)
                .await?
                .unwrap_or_default(),
        };
        let thing = |id: &str, name: String| ThingRef {
            id: MemoryThingId(id.to_owned()),
            name,
        };
        Ok(LinkView {
            id: MemoryLinkId(link.id.clone()),
            from: thing(&link.from, from_name),
            kind: link.kind.clone(),
            to: thing(&link.to, to_name),
            since: Timestamp(link.valid_from),
            until: link.valid_to.map(Timestamp),
            by: self.link_by(link).await?,
        })
    }
}

fn page_of_origin(origin: &str) -> Option<KnowledgePageId> {
    let (id, _) = origin.strip_prefix(PAGE_ORIGIN)?.split_once('#')?;
    KnowledgePageId::parse(id).ok()
}

// spec: ordin-desktop/components/MemoryService#SA-MPD-03
/// SA-MPD-03.
fn derived_origin(row: &RefRow) -> String {
    match &row.origin_ref {
        Some(origin) => origin.clone(),
        None => format!("fact:{}", row.id),
    }
}
