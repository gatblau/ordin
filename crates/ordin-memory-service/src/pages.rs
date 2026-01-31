//! Knowledge pages (*Internal Logic* §8 and rule R, with the ADR-111 and ADR-112 changes). C-35 holds:
//! a recalled fact carries its page through its `knowledge-page:` origin, so the fallback (no page
//! indexing) is not taken.

use std::time::Duration;

use memory::types::api::{EntityAssertion, LinkAssertion};
use memory::types::domain::Source;
use memory::write_pipeline::source_id;
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, TeamFolderId, WorkspaceId};
use serde_json::json;

use crate::error::MemoryError;
use crate::model::{FactState, MemoryPlace, PageGraph, PageSection, RememberReceipt};
use crate::remember::{RowSeed, Write, PAGE_ORIGIN};
use crate::service::{new_correlation_id, store_error_of, Inner, MemoryService};
use crate::store::{self, RefRow};

/// How long a batch waits for the write bucket in all (see `write_paced`).
const PACE_LIMIT: Duration = Duration::from_secs(60);
const PACE_STEP: Duration = Duration::from_millis(120);

/// §4 step 3's page key.
// `page:<page_id>:<version>:<n>` names no folder, and rule R gives a read copy the
// workspace's owner, so a page a workspace holds and also reads at one version would be one fact and
// one row, and removing either copy would remove both. A read copy's key names its folder.
fn section_key(
    folder: Option<TeamFolderId>,
    page_id: KnowledgePageId,
    version: &str,
    n: usize,
) -> String {
    match folder {
        Some(folder) => format!("page:{page_id}:{folder}:{version}:{n}"),
        None => format!("page:{page_id}:{version}:{n}"),
    }
}

impl Inner {
    /// A write of a batch. A page takes up to 200 writes at once and the bucket's burst is 10
    /// (`memory::api::ratelimit::WRITE_BURST`), so a batch waits for tokens where a single caller
    /// would be told to try again.
    pub(crate) async fn write_paced(
        &self,
        make: impl Fn() -> Write,
    ) -> Result<RememberReceipt, MemoryError> {
        let started = std::time::Instant::now();
        loop {
            match self.write(make(), &new_correlation_id()).await {
                Err(error)
                    if error.code() == "MEM_RATE_LIMITED" && started.elapsed() < PACE_LIMIT =>
                {
                    tokio::time::sleep(PACE_STEP).await;
                }
                other => return other,
            }
        }
    }

    /// §8 step 4: a row is retired now, or, when its write has not landed yet, by the reconciler.
    pub(crate) async fn retire_row(&self, row: &RefRow, key: String) -> Result<(), MemoryError> {
        let place = row.place()?;
        let started = std::time::Instant::now();
        let stored = loop {
            match self.retire_fact(place, &row.memory_fact_id, &key).await {
                Err(error)
                    if error.code() == "MEM_RATE_LIMITED" && started.elapsed() < PACE_LIMIT =>
                {
                    tokio::time::sleep(PACE_STEP).await;
                }
                other => break other?,
            }
        };
        let settled = stored.then(Timestamp::now);
        store::set_state(&self.db, row.id, FactState::Replaced, settled).await
    }

    /// §8 steps 1 to 5 and rule R.
    async fn index_sections(
        &self,
        workspace_id: WorkspaceId,
        folder: Option<TeamFolderId>,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        self.model_gate()?;
        if !(1..=200).contains(&sections.len()) {
            tracing::error!(target: "ordin::memory", "memory.page_refused");
            return Err(MemoryError::invalid(
                "MEM_PAGE_SECTIONS_INVALID",
                "sections",
            ));
        }
        for section in &sections {
            if section.heading.chars().count() > 200
                || !(1..=4000).contains(&section.text.chars().count())
            {
                tracing::error!(target: "ordin::memory", "memory.page_refused");
                return Err(MemoryError::invalid("MEM_PAGE_SECTION_INVALID", "section"));
            }
        }
        if !(1..=256).contains(&version.chars().count()) {
            return Err(MemoryError::invalid("MEM_SOURCE_INVALID", "version"));
        }

        // Step 2.
        let previous = store::page_rows(&self.db, workspace_id, folder, page_id).await?;

        let place = MemoryPlace::Workspace(workspace_id);
        let owner = Inner::owner_of(place);
        let collection = folder.map(|f| format!("tk:{f}"));
        let title_entity = EntityAssertion {
            name: graph.title.clone(),
            aliases: graph.other_names.clone(),
        };
        let (linked, dropped): (Vec<_>, Vec<_>) = graph
            .links_to
            .iter()
            .enumerate()
            .partition(|(at, _)| *at < 15);
        if !dropped.is_empty() {
            tracing::warn!(target: "ordin::memory", dropped = dropped.len(), "memory.page_links_dropped");
        }

        // Step 3.
        let mut written = Vec::with_capacity(sections.len());
        for (at, section) in sections.iter().enumerate() {
            let n = at + 1;
            let origin_ref = format!("{PAGE_ORIGIN}{page_id}#{n}");
            // spec: ordin-desktop/components/MemoryService#SA-MKA-01
            // SA-MKA-01: repeated at every index, so a changed tier takes effect.
            let source = Source {
                id: source_id(&owner, &origin_ref),
                origin_ref: origin_ref.clone(),
                modification_marker: Some(version.clone()),
                trust_signal: graph.standing.tier.trust_signal(),
                owner: owner.clone(),
                collection: collection.clone().unwrap_or_default(),
            };
            self.storage
                .put_source(&source)
                .await
                .map_err(store_error_of)?;

            let mut entities = vec![title_entity.clone()];
            let mut links = Vec::new();
            if n == 1 {
                for (_, (_, title)) in &linked {
                    entities.push(EntityAssertion {
                        name: title.clone(),
                        aliases: Vec::new(),
                    });
                    links.push(LinkAssertion {
                        from: graph.title.clone(),
                        kind: "related to".to_owned(),
                        to: title.clone(),
                        ended: false,
                        confidence: None,
                    });
                }
            }
            let mut seed = RowSeed::of("knowledge_page", "knowledge_service");
            // spec: ordin-desktop/components/MemoryService#SA-LINK-01
            seed.author = Some("knowledge-pages".to_owned());
            seed.origin_ref = Some(origin_ref.clone());
            seed.origin_version = Some(version.clone());
            seed.read_folder_id = folder;
            seed.may_be_out_of_date = graph.standing.past_stale_after;
            seed.page_status = Some(graph.standing.status);
            seed.page_tier = Some(graph.standing.tier);
            let receipt = self
                .write_paced(|| Write {
                    place,
                    context: self.place_context(place),
                    key: section_key(folder, page_id, &version, n),
                    statement: section.text.clone(),
                    about: (!section.heading.is_empty()).then(|| section.heading.clone()),
                    memory_class: "semantic",
                    fact_class: None,
                    source: Some((origin_ref.clone(), Some(version.clone()))),
                    visibility: "team-shared",
                    collection: collection.clone(),
                    entities: entities.clone(),
                    links: links.clone(),
                    made_from: Vec::new(),
                    row: seed.clone(),
                })
                .await?;
            written.push(receipt.id);
        }

        // Step 4.
        for row in &previous {
            if !written.contains(&row.id) {
                self.retire_row(row, format!("page-retire:{}", row.id))
                    .await?;
            }
        }

        // Step 5.
        self.journal(
            "page_indexed",
            None,
            None,
            Some(workspace_id),
            "A knowledge page was indexed.",
            json!({
                "page_id": page_id.to_string(),
                "sections": sections.len(),
                "version": version,
            }),
        )
        .await;
        tracing::info!(
            target: "ordin::memory",
            page_id = %page_id,
            sections = sections.len(),
            version = %version,
            "memory.page_indexed"
        );
        Ok(())
    }
}

impl MemoryService {
    /// §8. C-35 holds, so sections are written.
    pub async fn index_page_sections(
        &self,
        workspace_id: WorkspaceId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        self.inner
            .index_sections(workspace_id, None, page_id, sections, version, graph)
            .await
    }

    /// Rule R.
    pub async fn index_read_page_sections(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TeamFolderId,
        page_id: KnowledgePageId,
        sections: Vec<PageSection>,
        version: String,
        graph: PageGraph,
    ) -> Result<(), MemoryError> {
        self.inner
            .index_sections(
                workspace_id,
                Some(folder_id),
                page_id,
                sections,
                version,
                graph,
            )
            .await
    }

    /// §8: retire every `pending` or `kept` row of the page in this workspace and folder (`None`: the
    /// workspace's own); idempotent.
    pub async fn remove_page(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TeamFolderId>,
        page_id: KnowledgePageId,
    ) -> Result<(), MemoryError> {
        let inner = &self.inner;
        inner.gate()?;
        let rows = store::page_rows(&inner.db, workspace_id, folder_id, page_id).await?;
        for row in &rows {
            inner
                .retire_row(row, format!("page-retire:{}", row.id))
                .await?;
        }
        inner
            .journal(
                "page_removed",
                None,
                None,
                Some(workspace_id),
                "A knowledge page was removed from memory.",
                json!({ "page_id": page_id.to_string() }),
            )
            .await;
        tracing::info!(target: "ordin::memory", page_id = %page_id, "memory.page_removed");
        Ok(())
    }
}
