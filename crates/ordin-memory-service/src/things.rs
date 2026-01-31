//! The person's view of things and links (rules K, L, W, X, Y, Z) and a knowledge scope's
//! most-connected things (AI step 6, AI step 7).

use memory::service::{CorrectLinkRequest, EntityDetail, EntityDetailRequest, ListEntitiesRequest};
use memory::types::domain::Fact;
use ordin_types::time::Timestamp;
use ordin_types::{MemoryLinkId, MemoryThingId, ScopeId, WorkspaceId};
use serde_json::json;
use uuid::Uuid;

use crate::config::TENANT;
use crate::db::{like_prefix, q};
use crate::error::{is_not_found, map_call_error, MemoryError};
use crate::model::{
    FactState, LinkForgetReceipt, LinkView, MemoryPlace, ThingCount, ThingDetail,
    ThingForgetReceipt, ThingPage, ThingSummary, ThingView,
};
use crate::remember::sha256_hex;
use crate::service::{new_correlation_id, Inner, MemoryService};
use crate::store::{self, RefRow};
use crate::views::{Names, ViewNote};

/// SA-AMEM-09.
const SUMMARIES_OVER: usize = 1_000;

impl Inner {
    /// The wrong place for the program → `MEM_NOT_SUPPORTED`.
    fn check_place(&self, place: MemoryPlace) -> Result<(), MemoryError> {
        let fits = match place {
            MemoryPlace::Workspace(_) => self.is_workspace_program(),
            MemoryPlace::Scope(_) => self.program.is_agent_profile(),
        };
        if fits {
            Ok(())
        } else {
            Err(MemoryError::not_supported())
        }
    }

    async fn explained_in(
        &self,
        place: MemoryPlace,
        name: &str,
        aliases: &[String],
    ) -> Option<crate::model::PageLabel> {
        let (MemoryPlace::Workspace(workspace_id), Some(pages)) = (place, self.pages()) else {
            return None;
        };
        let mut names = vec![name.to_owned()];
        names.extend(aliases.iter().cloned());
        pages.page_explaining(workspace_id, &names).await
    }

    async fn detail(
        &self,
        place: MemoryPlace,
        id: &MemoryThingId,
    ) -> Result<EntityDetail, MemoryError> {
        let guard = self.auth.mint(self.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        self.service
            .entity_detail(
                guard.cx(&correlation_id, None),
                EntityDetailRequest { id: id.0.clone() },
            )
            .await
            .map(|result| result.data)
            .map_err(|error| {
                if is_not_found(&error) {
                    MemoryError::thing_not_found()
                } else {
                    map_call_error(error)
                }
            })
    }

    async fn thing_view(
        &self,
        place: MemoryPlace,
        detail: &EntityDetail,
    ) -> Result<ThingView, MemoryError> {
        let entity = &detail.entity;
        Ok(ThingView {
            id: MemoryThingId(entity.id.clone()),
            name: entity.canonical_name.clone(),
            other_names: entity.aliases.clone(),
            fact_count: detail.facts.len() as u32,
            link_count: detail.links.iter().filter(|l| l.valid_to.is_none()).count() as u32,
            explained_in: self
                .explained_in(place, &entity.canonical_name, &entity.aliases)
                .await,
        })
    }

    /// What `forget_entity` removes for this caller: its own facts naming the thing, of any
    /// validity, and its own links touching it. A teammate's rows stay, so they are not counted.
    // spec: ordin-desktop/components/MemoryService#SA-AMEM-09
    async fn own_counts(
        &self,
        place: MemoryPlace,
        id: &MemoryThingId,
    ) -> Result<(Vec<String>, u32), MemoryError> {
        let user = store::scope_key(place);
        let facts = self
            .db
            .all(
                q(
                    "SELECT f.id AS id FROM mem_fact_entity e JOIN mem_fact f ON f.id = e.fact_id \
                   WHERE e.entity_id = $1 AND f.tenant = $2 AND f.owner_user = $3 ORDER BY f.id",
                )
                .text(id.0.clone())
                .text(TENANT)
                .text(user.clone()),
            )
            .await?
            .iter()
            .map(|row| row.text("id"))
            .collect::<Result<Vec<_>, _>>()?;
        let links = store::count(
            &self.db,
            q(
                "SELECT COUNT(*) AS n FROM mem_relationship WHERE tenant = $1 AND owner_user = $2 \
               AND (from_ent = $3 OR to_ent = $4)",
            )
            .text(TENANT)
            .text(user)
            .text(id.0.clone())
            .text(id.0.clone()),
        )
        .await?;
        Ok((facts, links as u32))
    }

    /// Rule Y's counts over the facts just read.
    async fn thing_count(
        &self,
        place: MemoryPlace,
        id: &MemoryThingId,
    ) -> Result<(ThingCount, Vec<String>), MemoryError> {
        // The component's check: the thing must be readable here.
        let guard = self.auth.mint(self.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        self.service
            .count_entity(guard.cx(&correlation_id, None), &id.0)
            .await
            .map_err(|error| {
                if is_not_found(&error) {
                    MemoryError::thing_not_found()
                } else {
                    map_call_error(error)
                }
            })?;
        drop(guard);
        let (facts, links) = self.own_counts(place, id).await?;
        let first: Vec<String> = facts.iter().take(SUMMARIES_OVER).cloned().collect();
        let rows = store::refs_by_facts(&self.db, &first).await?;
        let origins: Vec<_> = rows.iter().map(|r| r.id).collect();
        let summaries = store::summaries_of(&self.db, &origins).await?;
        Ok((
            ThingCount {
                facts: facts.len() as u32,
                links,
                summaries,
            },
            facts,
        ))
    }

    /// The place of a link: `mem_relationship.owner_team`, by the one-row read of SA-AMEM-06.
    async fn link_place(&self, id: &MemoryLinkId) -> Result<MemoryPlace, MemoryError> {
        let row = self
            .db
            .first(q("SELECT owner_team FROM mem_relationship WHERE id = $1").text(id.0.clone()))
            .await?
            .ok_or_else(MemoryError::link_not_found)?;
        let team = row.opt_text("owner_team")?.unwrap_or_default();
        if let Some(raw) = team.strip_prefix("ws:") {
            return Uuid::parse_str(raw)
                .map(|u| MemoryPlace::Workspace(WorkspaceId(u)))
                .map_err(|_| MemoryError::link_not_found());
        }
        if let Some(raw) = team.strip_prefix("scope:") {
            return Uuid::parse_str(raw)
                .map(|u| MemoryPlace::Scope(ScopeId(u)))
                .map_err(|_| MemoryError::link_not_found());
        }
        Err(MemoryError::link_not_found())
    }

    /// AI steps 6 and 7's shared tail: `entity_detail` per entity, so the read filter still applies.
    async fn summaries_from(
        &self,
        place: MemoryPlace,
        rows: Vec<(String, u32, u32)>,
    ) -> Result<Vec<ThingSummary>, MemoryError> {
        let mut out = Vec::with_capacity(rows.len());
        let context = self.place_context(place);
        for (id, link_count, fact_count) in rows {
            let entity =
                memory::types::ports::MemoryStore::get_entity(self.storage.as_ref(), &context, &id)
                    .await
                    .map_err(crate::service::store_error_of)?;
            if let Some(entity) = entity {
                out.push(ThingSummary {
                    id: MemoryThingId(entity.id),
                    name: entity.canonical_name,
                    other_names: entity.aliases.into_iter().take(32).collect(),
                    link_count,
                    fact_count,
                });
            }
        }
        Ok(out)
    }
}

fn parse_top(rows: Vec<crate::db::R>) -> Result<Vec<(String, u32, u32)>, MemoryError> {
    rows.iter()
        .map(|row| {
            Ok((
                row.text("id")?,
                row.int("link_count")? as u32,
                row.int("fact_count")? as u32,
            ))
        })
        .collect()
}

impl MemoryService {
    /// Rule K.
    pub async fn list_things(
        &self,
        place: MemoryPlace,
        search: Option<String>,
        cursor: Option<String>,
    ) -> Result<ThingPage, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        if search
            .as_deref()
            .is_some_and(|s| !(1..=200).contains(&s.chars().count()))
        {
            return Err(MemoryError::invalid("MEM_SEARCH_INVALID", "search"));
        }
        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let page = inner
            .service
            .list_entities(
                guard.cx(&correlation_id, None),
                ListEntitiesRequest { search, cursor },
            )
            .await
            .map_err(map_call_error)?
            .data;
        drop(guard);
        let mut items = Vec::with_capacity(page.items.len());
        for item in page.items {
            items.push(ThingView {
                explained_in: inner
                    .explained_in(place, &item.entity.canonical_name, &item.entity.aliases)
                    .await,
                id: MemoryThingId(item.entity.id),
                name: item.entity.canonical_name,
                other_names: item.entity.aliases,
                fact_count: item.fact_count,
                link_count: item.link_count,
            });
        }
        Ok(ThingPage {
            items,
            next_cursor: page.next_cursor,
        })
    }

    /// Rule L.
    pub async fn get_thing(
        &self,
        place: MemoryPlace,
        id: MemoryThingId,
    ) -> Result<ThingDetail, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        let detail = inner.detail(place, &id).await?;
        let context = inner.place_context(place);
        let ids: Vec<String> = detail.facts.iter().map(|f| f.id.clone()).collect();
        let rows = store::refs_by_facts(&inner.db, &ids).await?;
        let held: Vec<&Fact> = detail.facts.iter().collect();
        let origins = inner.origin_rows(&held).await?;
        let mut names = Names::new();
        for (id, name) in &detail.names {
            names.insert(id.clone(), Some(name.clone()));
        }
        let mut facts = Vec::new();
        for fact in detail.facts.iter().take(200) {
            let Some(row) = rows
                .iter()
                .find(|r| r.memory_fact_id == fact.id && r.state == FactState::Kept)
            else {
                continue;
            };
            facts.push(
                inner
                    .fact_view(
                        fact,
                        Some(row),
                        ViewNote::default(),
                        &context,
                        &mut names,
                        &origins,
                    )
                    .await?,
            );
        }
        let mut links: Vec<LinkView> = Vec::new();
        for link in detail.links.iter().take(200) {
            links.push(
                inner
                    .link_view(link, None, None, &context, &mut names)
                    .await?,
            );
        }
        Ok(ThingDetail {
            thing: inner.thing_view(place, &detail).await?,
            facts,
            links,
        })
    }

    /// Rule W.
    pub async fn correct_link(
        &self,
        id: MemoryLinkId,
        kind: String,
        from: MemoryThingId,
        to: MemoryThingId,
    ) -> Result<LinkView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !(1..=64).contains(&kind.trim().chars().count()) || from == to {
            return Err(MemoryError::invalid("MEM_LINKS_INVALID", "kind"));
        }
        let place = inner.link_place(&id).await?;
        let context = inner.place_context(place);
        let guard = inner.auth.mint(context.clone()).await?;
        let correlation_id = new_correlation_id();
        let key = format!(
            "correct-link:{}",
            sha256_hex(&format!("{}\n{kind}\n{}\n{}", id.0, from.0, to.0))
        );
        let link = inner
            .service
            .correct_link(
                guard.cx(&correlation_id, Some(&key)),
                CorrectLinkRequest {
                    id: id.0.clone(),
                    kind,
                    from: from.0.clone(),
                    to: to.0.clone(),
                },
            )
            .await
            .map_err(|error| {
                if is_not_found(&error) {
                    // The component says the same for the link and for a thing: tell them apart.
                    MemoryError::thing_not_found()
                } else {
                    map_call_error(error)
                }
            })?
            .data;
        drop(guard);
        inner
            .journal(
                "link_corrected",
                None,
                None,
                match place {
                    MemoryPlace::Workspace(w) => Some(w),
                    MemoryPlace::Scope(_) => None,
                },
                "The person corrected a link.",
                json!({ "link_id": id.0, "new_link_id": link.id }),
            )
            .await;
        tracing::info!(target: "ordin::memory", link_id = %id.0, "memory.link_corrected");
        inner
            .link_view(&link, None, None, &context, &mut Names::new())
            .await
    }

    /// Rule X.
    pub async fn forget_link(&self, id: MemoryLinkId) -> Result<LinkForgetReceipt, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let place = inner.link_place(&id).await?;
        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let key = format!("forget-link:{}", id.0);
        let proof = inner
            .service
            .forget_link(guard.cx(&correlation_id, Some(&key)), &id.0)
            .await
            .map_err(|error| {
                if is_not_found(&error) {
                    MemoryError::link_not_found()
                } else {
                    map_call_error(error)
                }
            })?
            .data;
        drop(guard);
        inner
            .db
            .exec(q("DELETE FROM memory_recall_items WHERE link_id = $1").text(id.0.clone()))
            .await?;
        inner
            .journal(
                "link_forgotten",
                None,
                None,
                match place {
                    MemoryPlace::Workspace(w) => Some(w),
                    MemoryPlace::Scope(_) => None,
                },
                "The person forgot a link.",
                json!({ "link_id": id.0, "proof_digest": proof.digest }),
            )
            .await;
        tracing::info!(target: "ordin::memory", link_id = %id.0, "memory.link_forgotten");
        Ok(LinkForgetReceipt {
            id,
            forgotten_at: Timestamp(proof.deleted_at),
            proof_digest: proof.digest,
        })
    }

    /// Rule Y, the preview: what `forget_thing` would remove for this place.
    pub async fn count_thing(
        &self,
        place: MemoryPlace,
        id: MemoryThingId,
    ) -> Result<ThingCount, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        Ok(inner.thing_count(place, &id).await?.0)
    }

    /// Rule Y, as amended 2026-01-21: only the caller's facts are collected, and a thing a teammate
    /// still uses stays for them.
    pub async fn forget_thing(
        &self,
        place: MemoryPlace,
        id: MemoryThingId,
        expected: ThingCount,
    ) -> Result<ThingForgetReceipt, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        let (count, own_facts) = inner.thing_count(place, &id).await?;
        if count.facts != expected.facts || count.links != expected.links {
            return Err(MemoryError::Refused {
                code: "MEM_THING_COUNT_CHANGED",
            });
        }
        let rows = store::refs_by_facts(&inner.db, &own_facts).await?;
        let origins: Vec<_> = rows.iter().map(|r| r.id).collect();
        let summaries = inner.summaries_made_from(&origins).await?;

        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let key = format!(
            "forget-thing:{}",
            sha256_hex(&format!("{}\n{count:?}", id.0))
        );
        let proof = inner
            .service
            .forget_entity(guard.cx(&correlation_id, Some(&key)), &id.0)
            .await
            .map_err(map_call_error)?
            .data;
        drop(guard);

        // Every row of a collected fact, and of a summary whose fact no longer exists.
        let mut gone: Vec<_> = rows.iter().map(|r| r.id).collect();
        for summary in summaries {
            if inner.fact_of(&summary).await?.is_none() {
                gone.push(summary.id);
            }
        }
        let now = Timestamp::now();
        store::forget_rows(&inner.db, &gone, now).await?;

        inner
            .journal(
                "thing_forgotten",
                None,
                None,
                match place {
                    MemoryPlace::Workspace(w) => Some(w),
                    MemoryPlace::Scope(_) => None,
                },
                "The person forgot everything about a thing.",
                json!({
                    "thing_id": id.0,
                    "facts": proof.facts_removed,
                    "links": proof.links_removed,
                    "summaries": proof.derived_removed,
                    "proof_digest": proof.digest,
                    "thing_removed": proof.entity_removed,
                    "facts_remaining": proof.facts_remaining,
                    "links_remaining": proof.links_remaining,
                }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            thing_id = %id.0,
            facts = proof.facts_removed,
            links = proof.links_removed,
            summaries = proof.derived_removed,
            thing_removed = proof.entity_removed,
            facts_remaining = proof.facts_remaining,
            "memory.thing_forgotten"
        );
        Ok(ThingForgetReceipt {
            id,
            facts_removed: proof.facts_removed,
            links_removed: proof.links_removed,
            summaries_removed: proof.derived_removed,
            forgotten_at: now,
            proof_digest: proof.digest,
        })
    }

    /// Rule Z.
    pub async fn same_as(
        &self,
        place: MemoryPlace,
        keep: MemoryThingId,
        merge: MemoryThingId,
    ) -> Result<ThingView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        inner.check_place(place)?;
        if keep == merge {
            return Err(MemoryError::Refused {
                code: "MEM_SAME_THING",
            });
        }
        inner.detail(place, &keep).await?;
        inner.detail(place, &merge).await?;
        let guard = inner.auth.mint(inner.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        let key = format!(
            "same-as:{}",
            sha256_hex(&format!("{}\n{}", keep.0, merge.0))
        );
        inner
            .service
            .merge_entities(guard.cx(&correlation_id, Some(&key)), &keep.0, &merge.0)
            .await
            .map_err(map_call_error)?;
        drop(guard);
        inner
            .journal(
                "things_merged",
                None,
                None,
                match place {
                    MemoryPlace::Workspace(w) => Some(w),
                    MemoryPlace::Scope(_) => None,
                },
                "The person said two things are the same.",
                json!({ "kept": keep.0, "merged": merge.0 }),
            )
            .await;
        tracing::info!(target: "ordin::memory", kept = %keep.0, merged = %merge.0, "memory.things_merged");
        let detail = inner.detail(place, &keep).await?;
        inner.thing_view(place, &detail).await
    }

    // spec: ordin-desktop/components/MemoryService#SA-MKA-09
    /// AI step 6.
    pub async fn top_things(
        &self,
        place: MemoryPlace,
        limit: u16,
    ) -> Result<Vec<ThingSummary>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let MemoryPlace::Scope(_) = place else {
            return Err(MemoryError::not_supported());
        };
        if !inner.program.is_agent_profile() {
            return Err(MemoryError::not_supported());
        }
        if !(1..=1000).contains(&limit) {
            return Err(MemoryError::invalid("MEM_LIMIT_INVALID", "limit"));
        }
        let team = store::scope_key(place);
        let rows = inner
            .db
            .all(
                q("SELECT e.id AS id, e.canonical_name AS canonical_name, \
                     (SELECT COUNT(*) FROM mem_relationship r WHERE r.tenant = e.tenant \
                      AND r.valid_to IS NULL AND (r.from_ent = e.id OR r.to_ent = e.id)) \
                      AS link_count, \
                     (SELECT COUNT(*) FROM mem_fact_entity fe JOIN mem_fact f ON f.id = fe.fact_id \
                      WHERE fe.entity_id = e.id AND f.valid_to IS NULL) AS fact_count \
                   FROM mem_entity e WHERE e.tenant = $1 AND e.owner_team = $2 \
                   ORDER BY link_count DESC, fact_count DESC, e.canonical_name ASC LIMIT $3")
                .text(TENANT)
                .text(team)
                .int(i64::from(limit)),
            )
            .await?;
        inner.summaries_from(place, parse_top(rows)?).await
    }

    // spec: ordin-desktop/components/MemoryService#SA-MPD-04
    /// AI step 7.
    pub async fn top_things_within(
        &self,
        place: MemoryPlace,
        limit: u16,
        origin_prefixes: Vec<String>,
    ) -> Result<Vec<ThingSummary>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let MemoryPlace::Scope(_) = place else {
            return Err(MemoryError::not_supported());
        };
        if !inner.program.is_agent_profile() {
            return Err(MemoryError::not_supported());
        }
        let bad_limit = || MemoryError::invalid("MEM_LIMIT_INVALID", "limit");
        if !(1..=1000).contains(&limit)
            || !(1..=1000).contains(&origin_prefixes.len())
            || !origin_prefixes
                .iter()
                .all(|p| (1..=2049).contains(&p.chars().count()))
        {
            return Err(bad_limit());
        }
        let team = store::scope_key(place);
        let likes: Vec<String> = (0..origin_prefixes.len())
            .map(|at| format!("origin_ref LIKE ${} ESCAPE '\\'", at + 2))
            .collect();
        let next = origin_prefixes.len() + 2;
        let sql = format!(
            "WITH counted AS (SELECT memory_fact_id AS fact_id FROM memory_fact_refs \
               WHERE scope_key = $1 AND state IN ('pending', 'kept') AND ({})) \
             SELECT id, canonical_name, link_count, fact_count FROM (\
               SELECT e.id AS id, e.canonical_name AS canonical_name, \
                 (SELECT COUNT(DISTINCT r.id) FROM mem_relationship r \
                  JOIN mem_relationship_fact rf ON rf.relationship_id = r.id \
                    AND rf.role = 'asserted' \
                  JOIN counted c ON c.fact_id = rf.fact_id \
                  WHERE r.valid_to IS NULL AND (r.from_ent = e.id OR r.to_ent = e.id)) \
                  AS link_count, \
                 (SELECT COUNT(*) FROM mem_fact_entity fe JOIN counted c ON c.fact_id = fe.fact_id \
                  WHERE fe.entity_id = e.id) AS fact_count \
               FROM mem_entity e WHERE e.tenant = ${next} AND e.owner_team = ${}) t \
             WHERE fact_count > 0 \
             ORDER BY link_count DESC, fact_count DESC, canonical_name ASC LIMIT ${}",
            likes.join(" OR "),
            next + 1,
            next + 2
        );
        let mut st = q(sql).text(team.clone());
        for prefix in &origin_prefixes {
            st = st.text(like_prefix(prefix));
        }
        let rows = inner
            .db
            .all(st.text(TENANT).text(team).int(i64::from(limit)))
            .await?;
        inner.summaries_from(place, parse_top(rows)?).await
    }
}

impl Inner {
    /// The summary rows still kept that were made from `origins`.
    async fn summaries_made_from(
        &self,
        origins: &[ordin_types::MemoryFactId],
    ) -> Result<Vec<RefRow>, MemoryError> {
        if origins.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = q(format!(
            "SELECT DISTINCT fact_ref FROM memory_made_from WHERE origin_ref IN ({})",
            crate::db::placeholders(1, origins.len())
        ));
        for id in origins {
            st = st.uuid(id.0);
        }
        let ids: Vec<ordin_types::MemoryFactId> = self
            .db
            .all(st)
            .await?
            .iter()
            .map(|row| Ok(ordin_types::MemoryFactId(row.uuid("fact_ref")?)))
            .collect::<Result<_, MemoryError>>()?;
        Ok(store::refs_by_ids(&self.db, &ids)
            .await?
            .into_iter()
            .filter(|row| row.state == FactState::Kept)
            .collect())
    }
}
