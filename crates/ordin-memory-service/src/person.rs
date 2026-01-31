//! The person's actions on facts (*Internal Logic* §9, §10, and rules I, M, N, O, U, V, AA, AB) and
//! the small reads around them.

use std::collections::{HashSet, VecDeque};

use memory::error::AppError;
use memory::service::{CheckContentRequest, ContentCheck, FactHistoryRequest, FactOriginsRequest};
use memory::types::api::{ListFactsRequest, Triple};
use memory::types::domain::{Fact, MemoryClass};
use ordin_types::time::Timestamp;
use ordin_types::{MemoryFactId, MemoryRecallId, MemoryThingId, RunId, ScopeId, WorkspaceId};
use serde_json::{json, Value};

use crate::config::TENANT;
use crate::db::q;
use crate::error::{is_not_found, map_call_error, MemoryError};
use crate::model::{
    FactHistoryView, FactOrigin, FactPage, FactState, FactView, ForgetCount, ForgetReceipt,
    MemoryKind, MemoryPlace, MemoryUsedView, ObjectInput, OriginItem, OriginState, OriginsView,
    RememberReceipt, RememberedBy, RememberedInRun, RememberedItem, WiderUsed,
};
use crate::remember::{sha256_hex, RowSeed, Write};
use crate::service::{new_correlation_id, store_error_of, Inner, MemoryService};
use crate::store::{self, RefRow};
use crate::views::{text_of, Names, ViewNote};

impl Inner {
    /// A row that exists; anything else is `MEM_FACT_NOT_FOUND`.
    pub(crate) async fn row(&self, id: MemoryFactId) -> Result<RefRow, MemoryError> {
        store::ref_by_id(&self.db, id)
            .await?
            .ok_or_else(MemoryError::fact_not_found)
    }

    async fn kept_row(&self, id: MemoryFactId) -> Result<RefRow, MemoryError> {
        let row = self.row(id).await?;
        if row.state != FactState::Kept {
            return Err(MemoryError::fact_not_found());
        }
        Ok(row)
    }

    /// A stored fact of a row that must have one.
    pub(crate) async fn stored_fact(&self, row: &RefRow) -> Result<Fact, MemoryError> {
        self.fact_of(row)
            .await?
            .ok_or_else(MemoryError::fact_not_found)
    }

    /// `FactView` of a row whose new fact is still queued: what the row and the input know.
    fn pending_view(&self, row: &RefRow, statement: String, about: Option<String>) -> FactView {
        FactView {
            id: row.id,
            statement,
            about,
            workspace_id: row.workspace_id.filter(|_| !row.everywhere),
            came_from: FactOrigin::Person,
            written_at: row.written_at,
            may_be_out_of_date: false,
            page_id: None,
            page_section: None,
            relevance_permille: None,
            shared_by: None,
            shared_at: None,
            needs_check: false,
            folder: None,
            folder_description: None,
            set_by_company: false,
            adopted_from: None,
            adoption_mark: None,
            kind: MemoryKind::Known,
            true_from: row.written_at,
            true_until: None,
            replaces: row.replaces_id,
            fading: false,
            not_used_since: None,
            things: Vec::new(),
            made_from_count: None,
            page_tier: None,
            page_status: None,
            derived_from: Vec::new(),
            derived_origins: Vec::new(),
            triple: None,
            older_note: false,
            passage: false,
            conflicts_with: Vec::new(),
        }
    }

    // spec: ordin-desktop/components/MemoryService#SA-MREAD-01
    /// The write of `correct_fact` and `remember_everywhere`.
    #[allow(clippy::too_many_arguments)]
    async fn person_write(
        &self,
        old: &RefRow,
        statement: String,
        about: Option<String>,
        key: String,
        everywhere: bool,
        keeps_folder: bool,
        triple: Option<crate::remember::TripleSeed>,
    ) -> Result<RememberReceipt, MemoryError> {
        let place = old.place()?;
        let mut seed = RowSeed::of("person", "person");
        seed.replaces_id = Some(old.id);
        seed.everywhere = everywhere;
        // A new version of a fact is held no longer than the fact was.
        seed.retain_until = old.retain_until;
        let collection = if keeps_folder {
            seed.read_folder_id = old.read_folder_id;
            old.read_folder_id.map(|f| format!("tk:{f}"))
        } else {
            None
        };
        self.write(
            Write {
                place,
                context: self.place_context(place),
                key,
                statement,
                about,
                memory_class: "semantic",
                fact_class: old.fact_class.clone(),
                source: None,
                visibility: if everywhere {
                    "tenant-shared"
                } else {
                    "team-shared"
                },
                collection,
                entities: Vec::new(),
                links: Vec::new(),
                made_from: Vec::new(),
                row: seed,
                triple,
            },
            &new_correlation_id(),
        )
        .await
    }

    /// One row's erasure, by a forget or by retention. The job goes first, so that a write still
    /// queued, in backoff or held by a worker cannot land after the delete has looked for it.
    pub(crate) async fn erase_row(
        &self,
        row: &RefRow,
        key: &str,
        now: Timestamp,
    ) -> Result<Erased, MemoryError> {
        let place = row.place()?;
        let cancelled = self
            .storage
            .cancel_job(&Inner::owner_of(place), &row.job_id)
            .await
            .map_err(|error| map_call_error(AppError::Queue(error).into()))?;
        // spec: ordin-desktop/components/MemoryService#SA-TRUST-04
        self.storage
            .drop_held_of_fact(
                TENANT,
                Inner::owner_of(place).team.as_deref(),
                &row.memory_fact_id,
            )
            .await
            .map_err(store_error_of)?;
        match self.delete_fact(place, &row.memory_fact_id, key).await {
            Ok(proof) => {
                self.forget_with_derived(row, &proof.derived_removed, now)
                    .await?;
                Ok(Erased {
                    digest: proof.digest,
                    summaries: proof.derived_removed.len() as u32,
                    stored: true,
                    cancelled,
                })
            }
            Err(error) if is_not_found(&error) => {
                self.forget_with_derived(row, &[], now).await?;
                // The store holds no fact to prove gone, so the digest names what this erasure did
                // find: the write it stopped, or the row of a write that never landed.
                let what = if cancelled { "cancelled" } else { "not-stored" };
                Ok(Erased {
                    digest: sha256_hex(&format!("{what}:{}:{}", row.memory_fact_id, row.job_id)),
                    summaries: 0,
                    stored: false,
                    cancelled,
                })
            }
            Err(error) => Err(map_call_error(error)),
        }
    }

    /// The other versions of a row's fact, nearest last: rows joined by `replaces_id`, and facts
    /// joined by `supersedes`, which an imported row has no `replaces_id` for. A file can name any
    /// fact as what it supersedes, so a fact link counts only when the far fact points back.
    async fn versions_of(&self, row: &RefRow) -> Result<Vec<Version>, MemoryError> {
        let context = self.place_context(row.place()?);
        let read = |id: String| {
            let context = context.clone();
            async move {
                self.storage
                    .get_fact(&context, &id)
                    .await
                    .map_err(store_error_of)
            }
        };
        let mut seen = HashSet::from([row.memory_fact_id.clone()]);
        let mut queue = VecDeque::from([Version {
            fact: row.memory_fact_id.clone(),
            row: Some(row.clone()),
        }]);
        let mut found = Vec::new();
        while let Some(version) = queue.pop_front() {
            let mut near_rows = Vec::new();
            if let Some(current) = &version.row {
                if let Some(older) = current.replaces_id {
                    near_rows.extend(store::ref_by_id(&self.db, older).await?);
                }
                near_rows.extend(store::rows_replacing(&self.db, current.id).await?);
            }
            let mut near_facts = Vec::new();
            if let Some(fact) = read(version.fact.clone()).await? {
                for next in fact.supersedes.iter().chain(&fact.superseded_by) {
                    if seen.contains(next) {
                        continue;
                    }
                    if let Some(far) = read(next.clone()).await? {
                        let back = far.supersedes.as_deref() == Some(version.fact.as_str())
                            || far.superseded_by.as_deref() == Some(version.fact.as_str());
                        if back && far.owner == fact.owner {
                            near_facts.push(far.id);
                        }
                    }
                }
            }
            for near in near_rows {
                if near.scope_key == row.scope_key && seen.insert(near.memory_fact_id.clone()) {
                    queue.push_back(Version {
                        fact: near.memory_fact_id.clone(),
                        row: Some(near),
                    });
                }
            }
            for fact in near_facts {
                if seen.insert(fact.clone()) {
                    let row = store::ref_by_fact(&self.db, &fact).await?;
                    queue.push_back(Version { fact, row });
                }
            }
            found.push(version);
        }
        found.remove(0);
        found.reverse();
        Ok(found)
    }

    // spec: ordin-desktop/components/MemoryService#SA-AMEM-02
    /// §9 step 3 and rule G: the fact with every version of it, the rows, and what the proofs say went
    /// with them. The named row goes last and the farthest version first, so a retry after a failure
    /// still finds whatever is left.
    pub(crate) async fn forget_row(
        &self,
        row: &RefRow,
        prefix: &str,
    ) -> Result<Forgotten, MemoryError> {
        let place = row.place()?;
        let now = Timestamp::now();
        let mut digests = Vec::new();
        let mut summaries = 0;
        let mut cancelled = false;
        for version in self.versions_of(row).await? {
            match version.row {
                Some(other) if other.state == FactState::Forgotten => {}
                Some(other) => {
                    let key = format!("{prefix}:{}", other.id);
                    let erased = self.erase_row(&other, &key, now).await?;
                    digests.push(erased.digest);
                    summaries += erased.summaries;
                    cancelled |= erased.cancelled;
                }
                // A version with no row (rule M).
                None => {
                    let key = format!("{prefix}:{}", version.fact);
                    match self.delete_fact(place, &version.fact, &key).await {
                        Ok(proof) => {
                            self.forget_summaries(&proof.derived_removed, now).await?;
                            digests.push(proof.digest);
                            summaries += proof.derived_removed.len() as u32;
                        }
                        Err(error) if is_not_found(&error) => {}
                        Err(error) => return Err(map_call_error(error)),
                    }
                }
            }
        }
        let erased = self
            .erase_row(row, &format!("{prefix}:{}", row.id), now)
            .await?;
        digests.push(erased.digest);
        summaries += erased.summaries;
        let digest = match digests.as_slice() {
            [one] => one.clone(),
            many => {
                let mut sorted = many.to_vec();
                sorted.sort();
                sha256_hex(&sorted.join("\n"))
            }
        };
        Ok(Forgotten {
            at: now,
            digest,
            summaries,
            cancelled: cancelled || erased.cancelled,
        })
    }

    /// The rows of the summaries a delete took with it.
    async fn forget_summaries(
        &self,
        derived_removed: &[String],
        now: Timestamp,
    ) -> Result<(), MemoryError> {
        let ids: Vec<MemoryFactId> = store::refs_by_facts(&self.db, derived_removed)
            .await?
            .into_iter()
            .map(|r| r.id)
            .collect();
        store::forget_rows(&self.db, &ids, now).await
    }
}

/// One version of a fact: its stored id and, unless rule M's, its row.
struct Version {
    fact: String,
    row: Option<RefRow>,
}

/// What `Inner::erase_row` did.
pub(crate) struct Erased {
    pub(crate) digest: String,
    pub(crate) summaries: u32,
    /// The store held the fact.
    pub(crate) stored: bool,
    /// A write was stopped before it could land.
    pub(crate) cancelled: bool,
}

/// What `Inner::forget_row` did.
pub(crate) struct Forgotten {
    pub(crate) at: Timestamp,
    pub(crate) digest: String,
    pub(crate) summaries: u32,
    pub(crate) cancelled: bool,
}

impl MemoryService {
    /// §9 step 1.
    pub async fn list_facts(
        &self,
        workspace_id: WorkspaceId,
        cursor: Option<String>,
    ) -> Result<FactPage, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let place = MemoryPlace::Workspace(workspace_id);
        let context = inner.place_context(place);
        let guard = inner.auth.mint(context.clone()).await?;
        let correlation_id = new_correlation_id();
        let page = inner
            .service
            .list_facts(
                guard.cx(&correlation_id, None),
                ListFactsRequest {
                    cursor,
                    include_retired: false,
                },
            )
            .await
            .map_err(map_call_error)?
            .data;
        let ids: Vec<String> = page.facts.iter().map(|f| f.id.clone()).collect();
        let rows = store::refs_by_facts(&inner.db, &ids).await?;
        let held: Vec<&Fact> = page.facts.iter().collect();
        let origins = inner.origin_rows(&held).await?;
        let mut names = Names::new();
        let mut items = Vec::with_capacity(page.facts.len());
        let mut listed: Vec<&Fact> = Vec::with_capacity(page.facts.len());
        for fact in &page.facts {
            // §9 step 1 (ADR-118): passages are left out.
            // spec: ordin-desktop/components/MemoryService#MEM-TR-8
            if fact.fact_class.as_deref() == Some(crate::remember::PASSAGE_CLASS) {
                continue;
            }
            let Some(row) = rows
                .iter()
                .find(|r| r.memory_fact_id == fact.id && r.state == FactState::Kept)
            else {
                continue;
            };
            items.push(
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
            listed.push(fact);
        }
        inner.fill_conflicts(place, &listed, &mut items).await?;
        Ok(FactPage {
            items,
            next_cursor: page.next_cursor,
        })
    }

    /// §9 step 2: the whole call fails on an id of another workspace.
    pub async fn get_facts(
        &self,
        workspace_id: WorkspaceId,
        ids: Vec<MemoryFactId>,
    ) -> Result<Vec<FactView>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !(1..=100).contains(&ids.len()) {
            return Err(MemoryError::invalid("MEM_IDS_INVALID", "ids"));
        }
        let key = store::scope_key(MemoryPlace::Workspace(workspace_id));
        let mut views = Vec::with_capacity(ids.len());
        for id in ids {
            let row = inner.row(id).await?;
            if row.scope_key != key && !row.everywhere {
                return Err(MemoryError::fact_not_found());
            }
            let fact = inner.stored_fact(&row).await?;
            views.push(inner.view_of_row(&fact, &row).await?);
        }
        Ok(views)
    }

    /// §9 step 4 (replaced, ADR-118): the correction is the object, not a statement.
    // spec: ordin-desktop/components/MemoryService#MEM-TR-7
    pub async fn correct_fact(
        &self,
        id: MemoryFactId,
        object: ObjectInput,
    ) -> Result<FactView, MemoryError> {
        let inner = &self.inner;
        inner.model_gate()?;
        let old = inner.kept_row(id).await?;
        let fact = inner.stored_fact(&old).await?;
        if fact.fact_class.as_deref() == Some(crate::remember::PASSAGE_CLASS) {
            return Err(MemoryError::fact_not_found());
        }
        let Ok(triple) = serde_json::from_value::<Triple>(fact.content.clone()) else {
            return Err(MemoryError::Refused {
                code: "MEM_OLDER_NOTE",
            });
        };
        let place = old.place()?;
        let term =
            inner
                .live_term(place, &triple.predicate)
                .await?
                .ok_or(MemoryError::Refused {
                    code: "MEM_TERM_UNKNOWN",
                })?;
        if object.kind() != term.object {
            return Err(MemoryError::Refused {
                code: "MEM_TERM_OBJECT_MISMATCH",
            });
        }
        let new_object = crate::triples::triple_object(&object)
            .ok_or_else(|| MemoryError::invalid("MEM_TRIPLE_INVALID", "object"))?;
        if memory::triple::canonical_object(&new_object)
            == memory::triple::canonical_object(&triple.object)
            && memory::triple::object_name(&Triple {
                object: new_object.clone(),
                ..triple.clone()
            })
            .map(memory::text::normalise_name)
                == memory::triple::object_name(&triple).map(memory::text::normalise_name)
        {
            return Err(MemoryError::invalid("MEM_TRIPLE_INVALID", "object"));
        }
        let corrected = Triple {
            subject: triple.subject.clone(),
            predicate: term.key.clone(),
            object: new_object,
        };
        let content = serde_json::to_value(&corrected).map_err(|_| MemoryError::internal())?;
        let key = format!("correct:{id}:{}", sha256_hex(&content.to_string()));
        let seed = crate::remember::TripleSeed {
            content,
            single_valued: term.single_valued,
            changed: term.single_valued,
            ended: false,
            confidence: None,
            valid_from: None,
            subject_norm: memory::text::normalise_name(&corrected.subject),
            term_key: term.key.clone(),
        };
        let receipt = inner
            .person_write(&old, String::new(), None, key, false, true, Some(seed))
            .await?;
        let row = inner.row(receipt.id).await?;
        Ok(inner.pending_view(&row, String::new(), Some(corrected.subject)))
    }

    /// §9 step 3 with the ADR-111 change.
    pub async fn forget_fact(&self, id: MemoryFactId) -> Result<ForgetReceipt, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let row = inner.row(id).await?;
        if row.state == FactState::Forgotten {
            return Err(MemoryError::fact_not_found());
        }
        let forgotten = inner.forget_row(&row, "forget").await?;
        let fact_ref = id.to_string();
        inner
            .journal(
                "fact_forgotten",
                None,
                None,
                row.workspace_id,
                "The person forgot a fact.",
                json!({
                    "fact_ref": fact_ref,
                    "proof_digest": forgotten.digest,
                    "summaries": forgotten.summaries,
                    "cancelled": forgotten.cancelled,
                }),
            )
            .await;
        tracing::info!(target: "ordin::memory", fact_ref = id.to_string().as_str(), "memory.fact_forgotten");
        Ok(ForgetReceipt {
            id,
            forgotten_at: forgotten.at,
            proof_digest: forgotten.digest,
            summaries_removed: forgotten.summaries,
        })
    }

    /// §9 step 5.
    pub async fn remember_everywhere(&self, id: MemoryFactId) -> Result<FactView, MemoryError> {
        let inner = &self.inner;
        inner.model_gate()?;
        if !inner.is_workspace_program() {
            return Err(MemoryError::not_supported());
        }
        // MEM-SET-5: the person turned off using facts in other workspaces.
        // spec: ordin-desktop/components/MemoryService#MEM-SET-5
        if !inner.settings().use_everywhere {
            return Err(MemoryError::Refused {
                code: "MEM_EVERYWHERE_TURNED_OFF",
            });
        }
        let old = inner.kept_row(id).await?;
        if old.everywhere {
            return Err(MemoryError::Refused {
                code: "MEM_ALREADY_EVERYWHERE",
            });
        }
        let fact = inner.stored_fact(&old).await?;
        let statement = text_of(&fact, "statement").unwrap_or_default();
        let about = text_of(&fact, "subject");
        let key = format!("everywhere:{id}");
        // ADR-118: a triple is widened as its triple.
        let triple = match serde_json::from_value::<Triple>(fact.content.clone()) {
            Ok(triple) => {
                let single = match &old.term_key {
                    Some(k) => inner
                        .live_term(old.place()?, k)
                        .await?
                        .is_some_and(|t| t.single_valued),
                    None => false,
                };
                Some(crate::remember::TripleSeed {
                    content: fact.content.clone(),
                    single_valued: single,
                    changed: false,
                    ended: false,
                    confidence: None,
                    valid_from: None,
                    subject_norm: memory::text::normalise_name(&triple.subject),
                    term_key: triple.predicate.clone(),
                })
            }
            // SPEC-GAP-1: MEM-TR-7 says an older note is widened as before, but the component now
            // refuses a write whose content is not a triple unless it is a passage (memory
            // SA-TRIPLE-01), so its widened copy cannot be written; it is refused here instead.
            Err(_) => {
                return Err(MemoryError::Refused {
                    code: "MEM_OLDER_NOTE",
                })
            }
        };
        let receipt = inner
            .person_write(
                &old,
                statement.clone(),
                about.clone(),
                key,
                true,
                false,
                triple,
            )
            .await?;
        let row = inner.row(receipt.id).await?;
        Ok(inner.pending_view(&row, statement, about))
    }

    /// Rule I: **Keep as it is** on a fact waiting for a check.
    pub async fn keep_checked(&self, id: MemoryFactId) -> Result<FactView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let row = inner.kept_row(id).await?;
        let fact = inner.stored_fact(&row).await?;
        if !fact.pii_review {
            return Err(MemoryError::Refused {
                code: "MEM_NOT_HELD",
            });
        }
        let guard = inner.auth.mint(inner.place_context(row.place()?)).await?;
        let correlation_id = new_correlation_id();
        inner
            .service
            .clear_pii_review(guard.cx(&correlation_id, None), &row.memory_fact_id)
            .await
            .map_err(map_call_error)?;
        drop(guard);
        inner
            .journal(
                "fact_checked",
                None,
                None,
                row.workspace_id,
                "The person checked a held fact and kept it.",
                json!({ "fact_ref": id.to_string() }),
            )
            .await;
        tracing::info!(target: "ordin::memory", fact_ref = id.to_string().as_str(), "memory.fact_checked");
        let fact = inner.stored_fact(&row).await?;
        inner.view_of_row(&fact, &row).await
    }

    /// Rule O.
    pub async fn keep_fact(&self, id: MemoryFactId) -> Result<FactView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let row = inner.kept_row(id).await?;
        let fact = inner.stored_fact(&row).await?;
        if fact.memory_class != MemoryClass::Episodic {
            return Err(MemoryError::Refused {
                code: "MEM_ALREADY_KEPT",
            });
        }
        let guard = inner.auth.mint(inner.place_context(row.place()?)).await?;
        let correlation_id = new_correlation_id();
        let key = format!("keep:{id}");
        inner
            .service
            .keep_fact(guard.cx(&correlation_id, Some(&key)), &row.memory_fact_id)
            .await
            .map_err(map_call_error)?;
        drop(guard);
        inner
            .journal(
                "fact_kept",
                None,
                None,
                row.workspace_id,
                "The person kept a fact.",
                json!({ "fact_ref": id.to_string() }),
            )
            .await;
        tracing::info!(target: "ordin::memory", fact_ref = id.to_string().as_str(), "memory.fact_kept");
        let fact = inner.stored_fact(&row).await?;
        inner.view_of_row(&fact, &row).await
    }

    /// Rule M.
    pub async fn fact_history(&self, id: MemoryFactId) -> Result<FactHistoryView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let row = inner.row(id).await?;
        if row.state == FactState::Forgotten {
            return Err(MemoryError::fact_not_found());
        }
        let context = inner.place_context(row.place()?);
        let guard = inner.auth.mint(context.clone()).await?;
        let correlation_id = new_correlation_id();
        let history = inner
            .service
            .fact_history(
                guard.cx(&correlation_id, None),
                FactHistoryRequest {
                    id: row.memory_fact_id.clone(),
                },
            )
            .await
            .map_err(|error| {
                if is_not_found(&error) {
                    MemoryError::fact_not_found()
                } else {
                    map_call_error(error)
                }
            })?
            .data;
        drop(guard);
        let ids: Vec<String> = history.versions.iter().map(|v| v.fact.id.clone()).collect();
        let rows = store::refs_by_facts(&inner.db, &ids).await?;
        let held: Vec<&Fact> = history.versions.iter().map(|v| &v.fact).collect();
        let origins = inner.origin_rows(&held).await?;
        let mut names = Names::new();
        let mut versions = Vec::with_capacity(history.versions.len());
        let mut seen = HashSet::new();
        let mut links = Vec::new();
        for version in &history.versions {
            let row = rows.iter().find(|r| r.memory_fact_id == version.fact.id);
            versions.push(
                inner
                    .fact_view(
                        &version.fact,
                        row,
                        ViewNote::default(),
                        &context,
                        &mut names,
                        &origins,
                    )
                    .await?,
            );
            for (link, _role) in &version.links {
                if seen.insert(link.id.clone()) {
                    links.push(
                        inner
                            .link_view(link, None, None, &context, &mut names)
                            .await?,
                    );
                }
            }
        }
        Ok(FactHistoryView { versions, links })
    }

    /// Rule N.
    pub async fn fact_origins(&self, id: MemoryFactId) -> Result<OriginsView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let row = inner.row(id).await?;
        if row.state == FactState::Forgotten {
            return Err(MemoryError::fact_not_found());
        }
        let fact = inner.stored_fact(&row).await?;
        if fact.memory_class != MemoryClass::Consolidated {
            return Ok(OriginsView { items: Vec::new() });
        }
        let context = inner.place_context(row.place()?);
        let guard = inner.auth.mint(context.clone()).await?;
        let correlation_id = new_correlation_id();
        let origins = inner
            .service
            .fact_origins(
                guard.cx(&correlation_id, None),
                FactOriginsRequest {
                    id: row.memory_fact_id.clone(),
                },
            )
            .await
            .map_err(map_call_error)?
            .data;
        drop(guard);
        let mut items = Vec::with_capacity(origins.len());
        let mut names = Names::new();
        for origin in origins {
            let Some(fact) = origin.fact else {
                continue;
            };
            let row = store::ref_by_fact(&inner.db, &origin.id).await?;
            let view = inner
                .fact_view(
                    &fact,
                    row.as_ref(),
                    ViewNote::default(),
                    &context,
                    &mut names,
                    &Default::default(),
                )
                .await?;
            items.push(OriginItem {
                fact: view,
                state: if origin.ended {
                    OriginState::NoLongerTrue
                } else {
                    OriginState::Current
                },
            });
        }
        Ok(OriginsView { items })
    }

    /// Rule AA.
    pub async fn forget_count(&self, id: MemoryFactId) -> Result<ForgetCount, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let row = inner.kept_row(id).await?;
        let mut ids = vec![row.id];
        ids.extend(
            inner
                .versions_of(&row)
                .await?
                .into_iter()
                .filter_map(|version| version.row.map(|r| r.id)),
        );
        Ok(ForgetCount {
            summaries: store::summaries_of(&inner.db, &ids).await?,
        })
    }

    /// Rule U with the ADR-112/113 and ADR-115 changes.
    pub async fn memory_used(
        &self,
        place: MemoryPlace,
        recall_ids: Vec<MemoryRecallId>,
    ) -> Result<MemoryUsedView, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let invalid = || MemoryError::invalid("MEM_RECALL_IDS_INVALID", "recall_ids");
        let distinct: HashSet<MemoryRecallId> = recall_ids.iter().copied().collect();
        if !(1..=20).contains(&recall_ids.len()) || distinct.len() != recall_ids.len() {
            return Err(invalid());
        }
        let rows = store::recall_rows(&inner.db, &recall_ids).await?;
        let here = rows.len() == recall_ids.len()
            && rows.iter().all(|row| match place {
                MemoryPlace::Workspace(w) => row.workspace_id == Some(w),
                MemoryPlace::Scope(s) => row.scope_id == Some(s),
            });
        if !here {
            return Err(invalid());
        }
        let context = inner.place_context(place);
        let mut names = Names::new();
        let mut view = MemoryUsedView {
            facts: Vec::new(),
            links: Vec::new(),
            gone: 0,
            wider: Vec::new(),
            wider_unchecked: false,
        };
        let mut seen_facts = HashSet::new();
        let mut seen_links = HashSet::new();
        let mut seen_outside = HashSet::new();
        for id in &recall_ids {
            for item in store::recall_items(&inner.db, *id).await? {
                if item.fact_ref.0.is_nil() {
                    view.gone += 1;
                } else if item.item_kind == "fact" {
                    if !seen_facts.insert(item.fact_ref) {
                        continue;
                    }
                    match inner.used_fact(item.fact_ref).await? {
                        Some((fact, row)) => view.facts.push(
                            inner
                                .fact_view(
                                    &fact,
                                    Some(&row),
                                    ViewNote::default(),
                                    &context,
                                    &mut names,
                                    &Default::default(),
                                )
                                .await?,
                        ),
                        None => view.gone += 1,
                    }
                } else if let Some(link) = item.link_id {
                    if !seen_links.insert(link.clone()) {
                        continue;
                    }
                    let found = inner
                        .used_link(item.fact_ref, &link, &context, &mut names)
                        .await?;
                    match found {
                        Some(link) => view.links.push(link),
                        None => view.gone += 1,
                    }
                }
            }
            for row in store::recall_outside(&inner.db, *id).await? {
                let key = (
                    row.agent.clone(),
                    row.repository.clone(),
                    row.path.clone(),
                    row.commit.clone(),
                    row.section.clone(),
                );
                if seen_outside.insert(key) {
                    view.wider.push(WiderUsed {
                        agent: row.agent,
                        repository: row.repository,
                        path: row.path,
                        commit: row.commit,
                        section: row.section,
                        page_title: row.page_title,
                        as_of: row.as_of,
                        answer_id: row.answer_id.map(ordin_types::KnowledgeAnswerId),
                        more_found: row.more_found,
                    });
                }
            }
        }
        view.wider_unchecked = rows.iter().any(|row| {
            row.wider
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .is_some_and(|value| value["unchecked"] == Value::Bool(true))
        });
        Ok(view)
    }

    /// Rule V.
    pub async fn remembered_in_run(&self, run_id: RunId) -> Result<RememberedInRun, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let mut items = Vec::new();
        for row in store::rows_of_run(&inner.db, run_id).await? {
            let by = match (row.caller_kind.as_deref(), row.agent_instance_id) {
                (Some("agent"), Some(agent_instance_id)) => {
                    RememberedBy::Agent { agent_instance_id }
                }
                (Some("crew_member") | None, Some(agent_instance_id)) => {
                    RememberedBy::CrewMember { agent_instance_id }
                }
                _ => RememberedBy::Ordin,
            };
            let fact = match inner.fact_of(&row).await? {
                Some(fact) => inner.view_of_row(&fact, &row).await?,
                None => inner.pending_view(&row, String::new(), None),
            };
            items.push(RememberedItem { fact, by });
        }
        Ok(RememberedInRun { items })
    }

    /// Rule V′: the recall ids logged for a run, oldest first, at most 100.
    pub async fn recalls_of_run(&self, run_id: RunId) -> Result<Vec<MemoryRecallId>, MemoryError> {
        self.inner.gate()?;
        self.inner
            .db
            .all(
                q("SELECT id FROM memory_recalls WHERE run_id = $1 \
                   ORDER BY recalled_at, id LIMIT 100")
                .uuid(run_id.0),
            )
            .await?
            .iter()
            .map(|row| Ok(MemoryRecallId(row.uuid("id")?)))
            .collect()
    }

    /// §10: a cited source has a newer version.
    pub async fn source_changed(
        &self,
        origin_ref: &str,
        current_version: &str,
    ) -> Result<u32, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let rows = inner
            .db
            .exec(
                q(
                    "UPDATE memory_fact_refs SET may_be_out_of_date = $1 WHERE origin_ref = $2 \
                   AND origin_version <> $3 AND state IN ('pending', 'kept')",
                )
                .boolean(true)
                .text(origin_ref)
                .text(current_version),
            )
            .await?;
        tracing::info!(target: "ordin::memory", rows, "memory.source_changed");
        Ok(rows as u32)
    }

    /// Rule AB.
    pub async fn last_consolidated(
        &self,
        workspace_id: WorkspaceId,
        thing: MemoryThingId,
    ) -> Result<Option<Timestamp>, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let rows = inner
            .db
            .all(
                q(
                    "SELECT c.written_at AS written_at FROM memory_consolidations c \
                   JOIN memory_fact_refs r ON r.id = c.fact_ref \
                   WHERE c.workspace_id = $1 AND c.thing_id = $2 AND r.state = 'kept' \
                   ORDER BY c.written_at DESC LIMIT 1",
                )
                .uuid(workspace_id.0)
                .text(thing.0),
            )
            .await?;
        rows.first().map(|row| row.ts("written_at")).transpose()
    }

    /// Rule S.
    pub async fn count_workspace_facts(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<u32, MemoryError> {
        self.inner.gate()?;
        Ok(store::count(
            &self.inner.db,
            q(
                "SELECT COUNT(*) AS n FROM memory_fact_refs WHERE workspace_id = $1 \
               AND state IN ('pending', 'kept')",
            )
            .uuid(workspace_id.0),
        )
        .await? as u32)
    }

    /// Rule T: writes nothing.
    pub async fn check_content(
        &self,
        workspace_id: WorkspaceId,
        text: String,
    ) -> Result<ContentCheck, MemoryError> {
        self.inner.gate()?;
        if !(1..=8_000).contains(&text.chars().count()) {
            return Err(MemoryError::invalid("MEM_STATEMENT_INVALID", "text"));
        }
        self.inner
            .check_text(MemoryPlace::Workspace(workspace_id), text)
            .await
    }

    // spec: ordin-desktop/components/MemoryService#SA-RECON-03
    /// §11a: the number of facts `erase_scope(scope_id)` would remove, for a dry run.
    pub async fn count_scope(&self, scope_id: ScopeId) -> Result<u64, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.program.is_agent_profile() {
            return Err(MemoryError::not_supported());
        }
        store::count(
            &inner.db,
            q("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE scope_id = $1").uuid(scope_id.0),
        )
        .await
    }

    /// §11.
    pub async fn erase_scope(
        &self,
        scope_id: ScopeId,
    ) -> Result<crate::model::ScopeErasureReceipt, MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        if !inner.program.is_agent_profile() {
            return Err(MemoryError::not_supported());
        }
        let team = store::scope_key(MemoryPlace::Scope(scope_id));
        // §11 step 2 and rule P ask for the store's erasure and this service's rows in
        // one transaction. The port's `erase_scope` and `erase_collection` commit their own, so the
        // two steps run in that order: a failure between them leaves rows the next call clears, and
        // the store is the side that must be empty first.
        let proof = memory::types::ports::ScopeErasure::erase_scope(
            inner.storage.as_ref(),
            crate::config::TENANT,
            &team,
        )
        .await
        .map_err(crate::service::store_error_of)?;
        let rows_in_refs = store::count(
            &inner.db,
            q("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE scope_id = $1").uuid(scope_id.0),
        )
        .await?;
        let mut tx = inner.db.begin().await?;
        tx.exec(q("DELETE FROM memory_recalls WHERE scope_id = $1").uuid(scope_id.0))
            .await?;
        tx.exec(
            q("DELETE FROM memory_made_from WHERE fact_ref IN \
               (SELECT id FROM memory_fact_refs WHERE scope_id = $1) OR origin_ref IN \
               (SELECT id FROM memory_fact_refs WHERE scope_id = $2)")
            .uuid(scope_id.0)
            .uuid(scope_id.0),
        )
        .await?;
        tx.exec(
            q("UPDATE memory_fact_refs SET replaces_id = NULL WHERE scope_id = $1")
                .uuid(scope_id.0),
        )
        .await?;
        tx.exec(q("DELETE FROM memory_fact_refs WHERE scope_id = $1").uuid(scope_id.0))
            .await?;
        // spec: ordin-desktop/components/MemoryService#MEM-TOP-6
        tx.exec(q("DELETE FROM memory_thing_topics WHERE place_key = $1").text(team.clone()))
            .await?;
        tx.exec(q("DELETE FROM memory_topic_hints WHERE place_key = $1").text(team.clone()))
            .await?;
        tx.commit().await?;
        let left = store::count(
            &inner.db,
            q("SELECT COUNT(*) AS n FROM memory_fact_refs WHERE scope_id = $1").uuid(scope_id.0),
        )
        .await?;
        if left > 0 {
            return Err(MemoryError::delete_unverified());
        }
        let rows_removed = proof.total_removed + rows_in_refs;
        inner
            .journal(
                "scope_erased",
                None,
                None,
                None,
                "A conversation's memory was erased.",
                json!({
                    "scope_id": scope_id.to_string(),
                    "rows_removed": rows_removed,
                    "digest": proof.digest,
                }),
            )
            .await;
        tracing::info!(
            target: "ordin::memory",
            scope_id = scope_id.to_string().as_str(),
            rows_removed,
            "memory.scope_erased"
        );
        Ok(crate::model::ScopeErasureReceipt {
            scope_id,
            erased_at: Timestamp(proof.erased_at),
            rows_removed,
            digest: proof.digest,
        })
    }
}

impl Inner {
    /// `Service::check_content` under a place's context.
    pub(crate) async fn check_text(
        &self,
        place: MemoryPlace,
        text: String,
    ) -> Result<ContentCheck, MemoryError> {
        let guard = self.auth.mint(self.place_context(place)).await?;
        let correlation_id = new_correlation_id();
        self.service
            .check_content(
                guard.cx(&correlation_id, None),
                CheckContentRequest { text },
            )
            .await
            .map(|result| result.data)
            .map_err(map_call_error)
    }

    /// A fact item of rule U; None when it no longer counts as kept.
    async fn used_fact(&self, id: MemoryFactId) -> Result<Option<(Fact, RefRow)>, MemoryError> {
        let Some(row) = store::ref_by_id(&self.db, id).await? else {
            return Ok(None);
        };
        if matches!(
            row.state,
            FactState::Forgotten | FactState::Replaced | FactState::Retired
        ) {
            return Ok(None);
        }
        Ok(self.fact_of(&row).await?.map(|fact| (fact, row)))
    }

    /// A link item of rule U.
    // Rule U reads a link from `Service::fact_history` of the fact it reached, but a link
    // recall followed was asserted by another fact, so it is not in that fact's history. It is read
    // under the place's context, through the read filter; a link whose reached fact is gone is gone.
    async fn used_link(
        &self,
        reached: MemoryFactId,
        link: &str,
        context: &memory::types::scope::ScopeContext,
        names: &mut Names,
    ) -> Result<Option<crate::model::LinkView>, MemoryError> {
        if self.used_fact(reached).await?.is_none() {
            return Ok(None);
        }
        let found = self
            .storage
            .get_relationship(context, link)
            .await
            .map_err(crate::service::store_error_of)?;
        match found {
            Some(relationship) => self
                .link_view(&relationship, None, None, context, names)
                .await
                .map(Some),
            None => Ok(None),
        }
    }
}
