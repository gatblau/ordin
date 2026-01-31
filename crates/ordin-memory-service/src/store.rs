//! The statements of this module's own tables (*Data Model* §A and the amendments' additive steps).

use std::collections::HashSet;

use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::{
    AgentInstanceId, MemoryFactId, MemoryRecallId, RunId, ScopeId, TeamFolderId, WorkspaceId,
};
use uuid::Uuid;

use crate::db::{placeholders, q, Db, Tx, R};
use crate::error::MemoryError;
use crate::model::{FactState, MemoryPlace};
use crate::remember::PAGE_ORIGIN;

pub(crate) fn scope_key(place: MemoryPlace) -> String {
    match place {
        MemoryPlace::Workspace(w) => format!("ws:{w}"),
        MemoryPlace::Scope(s) => format!("scope:{s}"),
    }
}

pub(crate) fn state_text(state: FactState) -> &'static str {
    match state {
        FactState::Pending => "pending",
        FactState::Kept => "kept",
        FactState::NotKept => "not_kept",
        FactState::Retired => "retired",
        FactState::Forgotten => "forgotten",
        FactState::Replaced => "replaced",
    }
}

fn parse_state(raw: &str) -> Result<FactState, MemoryError> {
    Ok(match raw {
        "pending" => FactState::Pending,
        "kept" => FactState::Kept,
        "not_kept" => FactState::NotKept,
        "retired" => FactState::Retired,
        "forgotten" => FactState::Forgotten,
        "replaced" => FactState::Replaced,
        _ => return Err(MemoryError::internal()),
    })
}

pub(crate) fn tier_text(tier: PageTier) -> &'static str {
    match tier {
        PageTier::HumanReviewed => "human_reviewed",
        PageTier::MachineConfirmed => "machine_confirmed",
        PageTier::Unverified => "unverified",
    }
}

pub(crate) fn status_text(status: PageStatus) -> &'static str {
    match status {
        PageStatus::Stable => "stable",
        PageStatus::Draft => "draft",
        PageStatus::Deprecated => "deprecated",
    }
}

/// One `memory_fact_refs` row.
#[derive(Debug, Clone)]
pub(crate) struct RefRow {
    pub(crate) id: MemoryFactId,
    pub(crate) memory_fact_id: String,
    pub(crate) scope_key: String,
    pub(crate) workspace_id: Option<WorkspaceId>,
    pub(crate) scope_id: Option<ScopeId>,
    pub(crate) everywhere: bool,
    pub(crate) origin_kind: String,
    pub(crate) agent_instance_id: Option<AgentInstanceId>,
    pub(crate) run_id: Option<RunId>,
    pub(crate) origin_ref: Option<String>,
    pub(crate) origin_version: Option<String>,
    pub(crate) fact_class: Option<String>,
    pub(crate) job_id: String,
    pub(crate) state: FactState,
    pub(crate) may_be_out_of_date: bool,
    pub(crate) replaces_id: Option<MemoryFactId>,
    pub(crate) retain_until: Option<Timestamp>,
    pub(crate) written_at: Timestamp,
    pub(crate) settled_at: Option<Timestamp>,
    pub(crate) shared_by: Option<String>,
    pub(crate) shared_at: Option<Timestamp>,
    pub(crate) first_exported_at: Option<Timestamp>,
    pub(crate) team_imported: bool,
    pub(crate) read_folder_id: Option<TeamFolderId>,
    pub(crate) caller_kind: Option<String>,
    pub(crate) page_status: Option<PageStatus>,
    pub(crate) page_tier: Option<PageTier>,
    /// `normalise_name` of a triple's subject (MEM-TR-5).
    // spec: ordin-desktop/components/MemoryService#MEM-TR-5
    pub(crate) subject_norm: Option<String>,
    /// The term key of a triple's predicate.
    pub(crate) term_key: Option<String>,
}

impl RefRow {
    pub(crate) fn place(&self) -> Result<MemoryPlace, MemoryError> {
        match (self.workspace_id, self.scope_id) {
            (Some(w), None) => Ok(MemoryPlace::Workspace(w)),
            (None, Some(s)) => Ok(MemoryPlace::Scope(s)),
            _ => Err(MemoryError::internal()),
        }
    }

    /// The knowledge page a `knowledge-page:<id>#<n>` origin names. Only a row the page path wrote
    /// has one: what any other row's origin says is the caller's own text.
    pub(crate) fn page_id(&self) -> Option<ordin_types::KnowledgePageId> {
        if self.origin_kind != "knowledge_page" {
            return None;
        }
        let rest = self
            .origin_ref
            .as_deref()?
            .strip_prefix("knowledge-page:")?;
        let (id, _) = rest.split_once('#')?;
        ordin_types::KnowledgePageId::parse(id).ok()
    }
}

pub(crate) const REF_COLUMNS: &str = "id, memory_fact_id, scope_key, workspace_id, scope_id, \
    everywhere, origin_kind, agent_instance_id, run_id, origin_ref, origin_version, fact_class, \
    job_id, state, may_be_out_of_date, replaces_id, retain_until, written_at, settled_at, \
    shared_by, shared_at, first_exported_at, team_imported, read_folder_id, caller_kind, \
    page_status, page_tier, subject_norm, term_key";

fn parse_tier(raw: &str) -> Option<PageTier> {
    match raw {
        "human_reviewed" => Some(PageTier::HumanReviewed),
        "machine_confirmed" => Some(PageTier::MachineConfirmed),
        "unverified" => Some(PageTier::Unverified),
        _ => None,
    }
}

fn parse_status(raw: &str) -> Option<PageStatus> {
    match raw {
        "stable" => Some(PageStatus::Stable),
        "draft" => Some(PageStatus::Draft),
        "deprecated" => Some(PageStatus::Deprecated),
        _ => None,
    }
}

fn ref_of(row: &R) -> Result<RefRow, MemoryError> {
    Ok(RefRow {
        id: MemoryFactId(row.uuid("id")?),
        memory_fact_id: row.text("memory_fact_id")?,
        scope_key: row.text("scope_key")?,
        workspace_id: row.opt_uuid("workspace_id")?.map(WorkspaceId),
        scope_id: row.opt_uuid("scope_id")?.map(ScopeId),
        everywhere: row.boolean("everywhere")?,
        origin_kind: row.text("origin_kind")?,
        agent_instance_id: row.opt_uuid("agent_instance_id")?.map(AgentInstanceId),
        run_id: row.opt_uuid("run_id")?.map(RunId),
        origin_ref: row.opt_text("origin_ref")?,
        origin_version: row.opt_text("origin_version")?,
        fact_class: row.opt_text("fact_class")?,
        job_id: row.text("job_id")?,
        state: parse_state(&row.text("state")?)?,
        may_be_out_of_date: row.boolean("may_be_out_of_date")?,
        replaces_id: row.opt_uuid("replaces_id")?.map(MemoryFactId),
        retain_until: row.opt_ts("retain_until")?,
        written_at: row.ts("written_at")?,
        settled_at: row.opt_ts("settled_at")?,
        shared_by: row.opt_text("shared_by")?,
        shared_at: row.opt_ts("shared_at")?,
        first_exported_at: row.opt_ts("first_exported_at")?,
        team_imported: row.boolean("team_imported")?,
        read_folder_id: row.opt_uuid("read_folder_id")?.map(TeamFolderId),
        caller_kind: row.opt_text("caller_kind")?,
        page_status: row
            .opt_text("page_status")?
            .as_deref()
            .and_then(parse_status),
        page_tier: row.opt_text("page_tier")?.as_deref().and_then(parse_tier),
        subject_norm: row.opt_text("subject_norm")?,
        term_key: row.opt_text("term_key")?,
    })
}

fn refs_of(rows: Vec<R>) -> Result<Vec<RefRow>, MemoryError> {
    rows.iter().map(ref_of).collect()
}

const INSERT_REF: &str = "INSERT INTO memory_fact_refs (id, memory_fact_id, scope_key, \
    workspace_id, scope_id, everywhere, origin_kind, agent_instance_id, run_id, origin_ref, \
    origin_version, fact_class, job_id, state, may_be_out_of_date, replaces_id, retain_until, \
    written_at, settled_at, shared_by, shared_at, team_imported, read_folder_id, caller_kind, \
    page_status, page_tier, subject_norm, term_key) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, \
    $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)";

fn insert_q(row: &RefRow) -> crate::db::Q {
    q(INSERT_REF)
        .uuid(row.id.0)
        .text(row.memory_fact_id.clone())
        .text(row.scope_key.clone())
        .opt_uuid(row.workspace_id.map(|w| w.0))
        .opt_uuid(row.scope_id.map(|s| s.0))
        .boolean(row.everywhere)
        .text(row.origin_kind.clone())
        .opt_uuid(row.agent_instance_id.map(|a| a.0))
        .opt_uuid(row.run_id.map(|r| r.0))
        .opt_text(row.origin_ref.as_deref())
        .opt_text(row.origin_version.as_deref())
        .opt_text(row.fact_class.as_deref())
        .text(row.job_id.clone())
        .text(state_text(row.state))
        .boolean(row.may_be_out_of_date)
        .opt_uuid(row.replaces_id.map(|r| r.0))
        .opt_ts(row.retain_until)
        .ts(row.written_at)
        .opt_ts(row.settled_at)
        .opt_text(row.shared_by.as_deref())
        .opt_ts(row.shared_at)
        .boolean(row.team_imported)
        .opt_uuid(row.read_folder_id.map(|f| f.0))
        .opt_text(row.caller_kind.as_deref())
        .opt_text(row.page_status.map(status_text))
        .opt_text(row.page_tier.map(tier_text))
        .opt_text(row.subject_norm.as_deref())
        .opt_text(row.term_key.as_deref())
}

pub(crate) async fn insert_ref(db: &Db, row: &RefRow) -> Result<(), MemoryError> {
    db.exec(insert_q(row)).await.map(|_| ())
}

pub(crate) async fn insert_ref_tx(tx: &mut Tx, row: &RefRow) -> Result<(), MemoryError> {
    tx.exec(insert_q(row)).await.map(|_| ())
}

pub(crate) async fn ref_by_id(db: &Db, id: MemoryFactId) -> Result<Option<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE id = $1"
    ))
    .uuid(id.0);
    db.first(st).await?.as_ref().map(ref_of).transpose()
}

pub(crate) async fn ref_by_fact(db: &Db, fact: &str) -> Result<Option<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE memory_fact_id = $1"
    ))
    .text(fact);
    db.first(st).await?.as_ref().map(ref_of).transpose()
}

pub(crate) async fn refs_by_ids(db: &Db, ids: &[MemoryFactId]) -> Result<Vec<RefRow>, MemoryError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE id IN ({})",
        placeholders(1, ids.len())
    ));
    for id in ids {
        st = st.uuid(id.0);
    }
    refs_of(db.all(st).await?)
}

/// A person stands behind an agent's citation, so its fact may go to colleagues (SA-CITE-02).
pub(crate) async fn confirm_citation(
    db: &Db,
    id: MemoryFactId,
    now: Timestamp,
) -> Result<(), MemoryError> {
    db.exec(
        q(
            "INSERT INTO memory_citations_confirmed (fact_ref, confirmed_at) VALUES ($1, $2) \
           ON CONFLICT (fact_ref) DO NOTHING",
        )
        .uuid(id.0)
        .ts(now),
    )
    .await?;
    Ok(())
}

/// The confirmed citations of a scope.
pub(crate) async fn confirmed_citations(
    db: &Db,
    scope_key: &str,
) -> Result<std::collections::HashSet<MemoryFactId>, MemoryError> {
    db.all(
        q("SELECT c.fact_ref AS id FROM memory_citations_confirmed c \
           JOIN memory_fact_refs r ON r.id = c.fact_ref WHERE r.scope_key = $1")
        .text(scope_key.to_owned()),
    )
    .await?
    .iter()
    .map(|row| Ok(MemoryFactId(row.uuid("id")?)))
    .collect()
}

/// A scope's kept facts where an agent cites a source no person has confirmed, newest first.
pub(crate) async fn unconfirmed_citations(
    db: &Db,
    scope_key: &str,
) -> Result<Vec<MemoryFactId>, MemoryError> {
    db.all(
        q("SELECT r.id AS id FROM memory_fact_refs r \
           LEFT JOIN memory_citations_confirmed c ON c.fact_ref = r.id \
           WHERE r.scope_key = $1 AND r.state = 'kept' AND r.origin_kind = 'document' \
           AND r.agent_instance_id IS NOT NULL AND c.fact_ref IS NULL \
           ORDER BY r.written_at DESC LIMIT 1000")
        .text(scope_key.to_owned()),
    )
    .await?
    .iter()
    .map(|row| Ok(MemoryFactId(row.uuid("id")?)))
    .collect()
}

/// The summaries resting on `facts`, however many summaries lie between.
pub(crate) async fn summaries_resting_on(
    db: &Db,
    facts: &[MemoryFactId],
) -> Result<HashSet<MemoryFactId>, MemoryError> {
    let links: Vec<(MemoryFactId, MemoryFactId)> = db
        .all(q("SELECT fact_ref, origin_ref FROM memory_made_from"))
        .await?
        .iter()
        .map(|row| {
            Ok((
                MemoryFactId(row.uuid("fact_ref")?),
                MemoryFactId(row.uuid("origin_ref")?),
            ))
        })
        .collect::<Result<_, MemoryError>>()?;
    let mut reached: HashSet<MemoryFactId> = facts.iter().copied().collect();
    let mut summaries = HashSet::new();
    let mut grew = true;
    while grew {
        grew = false;
        for (summary, origin) in &links {
            if reached.contains(origin) && reached.insert(*summary) {
                summaries.insert(*summary);
                grew = true;
            }
        }
    }
    Ok(summaries)
}

/// The rows of `memory_fact_id`s, in one `SELECT` per call however many facts it holds.
pub(crate) async fn refs_by_facts(db: &Db, facts: &[String]) -> Result<Vec<RefRow>, MemoryError> {
    if facts.is_empty() {
        return Ok(Vec::new());
    }
    let mut st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE memory_fact_id IN ({})",
        placeholders(1, facts.len())
    ));
    for fact in facts {
        st = st.text(fact.clone());
    }
    refs_of(db.all(st).await?)
}

/// §6 step 1: oldest first. Rows retired before their write landed (`replaced`, not settled) are
/// read too, so the fact is retired when it arrives.
pub(crate) async fn unsettled(db: &Db, limit: i64) -> Result<Vec<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE state = 'pending' \
         OR (state = 'replaced' AND settled_at IS NULL) ORDER BY written_at, id LIMIT $1"
    ))
    .int(limit);
    refs_of(db.all(st).await?)
}

pub(crate) async fn set_state(
    db: &Db,
    id: MemoryFactId,
    state: FactState,
    settled_at: Option<Timestamp>,
) -> Result<(), MemoryError> {
    db.exec(
        q("UPDATE memory_fact_refs SET state = $1, settled_at = $2 WHERE id = $3")
            .text(state_text(state))
            .opt_ts(settled_at)
            .uuid(id.0),
    )
    .await
    .map(|_| ())
}

/// A row settles only from the state it was read in: a forget that overtook the read stands.
pub(crate) async fn settle(
    db: &Db,
    id: MemoryFactId,
    from: FactState,
    to: FactState,
    settled_at: Timestamp,
) -> Result<bool, MemoryError> {
    let changed = db
        .exec(
            q(
                "UPDATE memory_fact_refs SET state = $1, settled_at = $2 WHERE id = $3 \
               AND state = $4",
            )
            .text(state_text(to))
            .ts(settled_at)
            .uuid(id.0)
            .text(state_text(from)),
        )
        .await?;
    Ok(changed > 0)
}

/// The rows that replaced a row (`replaces_id`).
pub(crate) async fn rows_replacing(db: &Db, id: MemoryFactId) -> Result<Vec<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE replaces_id = $1"
    ))
    .uuid(id.0);
    refs_of(db.all(st).await?)
}

/// Every earlier version of a forgotten row of a scope, however far back.
pub(crate) async fn earlier_than_forgotten(
    db: &Db,
    scope: &str,
) -> Result<HashSet<MemoryFactId>, MemoryError> {
    let st = q("WITH RECURSIVE earlier(id) AS ( \
           SELECT replaces_id FROM memory_fact_refs WHERE scope_key = $1 \
             AND state = 'forgotten' AND replaces_id IS NOT NULL \
           UNION \
           SELECT r.replaces_id FROM memory_fact_refs r JOIN earlier e ON r.id = e.id \
             WHERE r.replaces_id IS NOT NULL \
         ) SELECT id FROM earlier")
    .text(scope);
    db.all(st)
        .await?
        .iter()
        .map(|row| Ok(MemoryFactId(row.uuid("id")?)))
        .collect()
}

/// The kept or pending rows of a page (§8 step 2) in one workspace, of the given folder or the
/// workspace's own.
pub(crate) async fn page_rows(
    db: &Db,
    workspace: WorkspaceId,
    folder: Option<TeamFolderId>,
    page: ordin_types::KnowledgePageId,
) -> Result<Vec<RefRow>, MemoryError> {
    let in_folder = if folder.is_some() {
        "read_folder_id = $3"
    } else {
        "read_folder_id IS NULL"
    };
    let mut st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE origin_kind = 'knowledge_page' \
         AND scope_key = $1 AND origin_ref LIKE $2 ESCAPE '\\' \
         AND state IN ('pending', 'kept') AND {in_folder}"
    ))
    .text(scope_key(MemoryPlace::Workspace(workspace)))
    .text(crate::db::like_prefix(&format!("{PAGE_ORIGIN}{page}#")));
    if let Some(folder) = folder {
        st = st.uuid(folder.0);
    }
    refs_of(db.all(st).await?)
}

/// The kept or pending rows of a scope whose origin begins with `prefix`, at most `limit`.
pub(crate) async fn rows_with_prefix(
    db: &Db,
    scope: &str,
    prefix: &str,
    limit: i64,
) -> Result<Vec<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE scope_key = $1 AND origin_ref LIKE $2 \
         ESCAPE '\\' AND state IN ('pending', 'kept') ORDER BY written_at, id LIMIT $3"
    ))
    .text(scope)
    .text(crate::db::like_prefix(prefix))
    .int(limit);
    refs_of(db.all(st).await?)
}

/// §7 step 1: a replaced version keeps the limit of the fact it was.
pub(crate) async fn rows_to_expire(
    db: &Db,
    now: Timestamp,
    limit: i64,
) -> Result<Vec<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE state IN ('kept', 'replaced') \
         AND retain_until IS NOT NULL AND retain_until <= $1 ORDER BY retain_until LIMIT $2"
    ))
    .ts(now)
    .int(limit);
    refs_of(db.all(st).await?)
}

pub(crate) async fn count(db: &Db, st: crate::db::Q) -> Result<u64, MemoryError> {
    let row = db.first(st).await?.ok_or_else(MemoryError::internal)?;
    Ok(u64::try_from(row.int("n")?).unwrap_or(0))
}

/// The rows of a run (rule V).
pub(crate) async fn rows_of_run(db: &Db, run: RunId) -> Result<Vec<RefRow>, MemoryError> {
    let st = q(format!(
        "SELECT {REF_COLUMNS} FROM memory_fact_refs WHERE run_id = $1 \
         AND state IN ('pending', 'kept') ORDER BY written_at, id LIMIT 500"
    ))
    .uuid(run.0);
    refs_of(db.all(st).await?)
}

/// §9 step 3 (ADR-111): the rows of `ids` become `forgotten`, and their recall-log items and
/// made-from rows go, in one transaction.
pub(crate) async fn forget_rows(
    db: &Db,
    ids: &[MemoryFactId],
    now: Timestamp,
) -> Result<(), MemoryError> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    for id in ids {
        // SA-AMEM-10 deletes a forgotten fact's log items, and the scenario *What Ordin
        // used* still counts it in `gone`. The item stays with the identifier cleared: nothing of
        // the fact is kept, and the place it held is what `gone` counts.
        tx.exec(
            q("UPDATE memory_recall_items SET fact_ref = $1 WHERE fact_ref = $2")
                .uuid(Uuid::nil())
                .uuid(id.0),
        )
        .await?;
        tx.exec(
            q("DELETE FROM memory_made_from WHERE fact_ref = $1 OR origin_ref = $2")
                .uuid(id.0)
                .uuid(id.0),
        )
        .await?;
        tx.exec(
            q("UPDATE memory_fact_refs SET state = 'forgotten', settled_at = $1 WHERE id = $2")
                .ts(now)
                .uuid(id.0),
        )
        .await?;
    }
    tx.commit().await
}

// spec: ordin-desktop/components/MemoryService#SA-AMEM-10
/// The recall log's parent row and items, in one transaction (§5 step 3 of the ADR-111 change).
pub(crate) struct NewRecall<'a> {
    pub(crate) id: MemoryRecallId,
    pub(crate) run_id: Option<RunId>,
    pub(crate) place: MemoryPlace,
    pub(crate) caller_kind: &'a str,
    pub(crate) agent_instance_id: Option<AgentInstanceId>,
    pub(crate) recalled_at: Timestamp,
    /// `{answered, unanswered, unchecked}`; None = not asked.
    pub(crate) wider: Option<String>,
    pub(crate) facts: Vec<MemoryFactId>,
    /// `(link id, first shown fact it reached)`.
    pub(crate) links: Vec<(String, MemoryFactId)>,
    pub(crate) outside: Vec<OutsideRow>,
}

/// One `memory_recall_wider` row.
#[derive(Debug, Clone)]
pub(crate) struct OutsideRow {
    pub(crate) agent: String,
    pub(crate) repository: String,
    pub(crate) path: String,
    pub(crate) commit: String,
    pub(crate) section: Option<String>,
    pub(crate) page_title: Option<String>,
    pub(crate) as_of: Timestamp,
    pub(crate) answer_id: Option<Uuid>,
    pub(crate) more_found: bool,
}

fn json_param(db: &Db, n: usize) -> String {
    if db.is_postgres() {
        format!("CAST(${n} AS jsonb)")
    } else {
        format!("${n}")
    }
}

pub(crate) async fn insert_recall(db: &Db, recall: NewRecall<'_>) -> Result<(), MemoryError> {
    let (workspace_id, scope_id) = match recall.place {
        MemoryPlace::Workspace(w) => (Some(w.0), None),
        MemoryPlace::Scope(s) => (None, Some(s.0)),
    };
    let mut tx = db.begin().await?;
    tx.exec(
        q(format!(
            "INSERT INTO memory_recalls (id, run_id, workspace_id, scope_id, caller_kind, \
             agent_instance_id, recalled_at, wider) VALUES ($1, $2, $3, $4, $5, $6, $7, {})",
            json_param(db, 8)
        ))
        .uuid(recall.id.0)
        .opt_uuid(recall.run_id.map(|r| r.0))
        .opt_uuid(workspace_id)
        .opt_uuid(scope_id)
        .text(recall.caller_kind)
        .opt_uuid(recall.agent_instance_id.map(|a| a.0))
        .ts(recall.recalled_at)
        .opt_text(recall.wider.as_deref()),
    )
    .await?;
    let mut position = 0i64;
    for fact in &recall.facts {
        tx.exec(
            q(
                "INSERT INTO memory_recall_items (recall_id, position, item_kind, fact_ref) \
               VALUES ($1, $2, 'fact', $3)",
            )
            .uuid(recall.id.0)
            .int(position)
            .uuid(fact.0),
        )
        .await?;
        position += 1;
    }
    for (link, fact) in &recall.links {
        tx.exec(
            q(
                "INSERT INTO memory_recall_items (recall_id, position, item_kind, fact_ref, \
               link_id) VALUES ($1, $2, 'link', $3, $4)",
            )
            .uuid(recall.id.0)
            .int(position)
            .uuid(fact.0)
            .text(link.clone()),
        )
        .await?;
        position += 1;
    }
    for (at, row) in recall.outside.iter().enumerate() {
        insert_outside_tx(&mut tx, recall.id, at as i64, row).await?;
    }
    tx.commit().await
}

async fn insert_outside_tx(
    tx: &mut Tx,
    recall: MemoryRecallId,
    position: i64,
    row: &OutsideRow,
) -> Result<(), MemoryError> {
    tx.exec(
        q(
            "INSERT INTO memory_recall_wider (recall_id, position, agent, repository, path, \
           commit_id, section, page_title, as_of, answer_id, more_found) \
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .uuid(recall.0)
        .int(position)
        .text(row.agent.clone())
        .text(row.repository.clone())
        .text(row.path.clone())
        .text(row.commit.clone())
        .opt_text(row.section.as_deref())
        .opt_text(row.page_title.as_deref())
        .ts(row.as_of)
        .opt_uuid(row.answer_id)
        .boolean(row.more_found),
    )
    .await
    .map(|_| ())
}

/// AG step 6: a later batch's outside facts, positions continuing, at most 50 in all.
pub(crate) async fn append_outside(
    db: &Db,
    recall: MemoryRecallId,
    rows: &[OutsideRow],
) -> Result<(), MemoryError> {
    let n = count(
        db,
        q("SELECT COUNT(*) AS n FROM memory_recall_wider WHERE recall_id = $1").uuid(recall.0),
    )
    .await? as i64;
    let mut tx = db.begin().await?;
    for (at, row) in rows.iter().enumerate() {
        let position = n + at as i64;
        if position >= 50 {
            break;
        }
        insert_outside_tx(&mut tx, recall, position, row).await?;
    }
    tx.commit().await
}

/// `memory_recalls.wider`, the JSON of `{answered, unanswered, unchecked}`.
pub(crate) async fn set_recall_wider(
    db: &Db,
    recall: MemoryRecallId,
    wider: &str,
) -> Result<(), MemoryError> {
    db.exec(
        q(format!(
            "UPDATE memory_recalls SET wider = {} WHERE id = $2",
            json_param(db, 1)
        ))
        .text(wider)
        .uuid(recall.0),
    )
    .await
    .map(|_| ())
}

/// One `memory_recalls` row, for the place check.
pub(crate) struct RecallRow {
    pub(crate) workspace_id: Option<WorkspaceId>,
    pub(crate) scope_id: Option<ScopeId>,
    pub(crate) wider: Option<String>,
}

pub(crate) async fn recall_rows(
    db: &Db,
    ids: &[MemoryRecallId],
) -> Result<Vec<RecallRow>, MemoryError> {
    let mut st = q(format!(
        "SELECT workspace_id, scope_id, CAST(wider AS TEXT) AS wider FROM memory_recalls \
         WHERE id IN ({})",
        placeholders(1, ids.len())
    ));
    for id in ids {
        st = st.uuid(id.0);
    }
    db.all(st)
        .await?
        .iter()
        .map(|row| {
            Ok(RecallRow {
                workspace_id: row.opt_uuid("workspace_id")?.map(WorkspaceId),
                scope_id: row.opt_uuid("scope_id")?.map(ScopeId),
                wider: row.opt_text("wider")?,
            })
        })
        .collect()
}

/// One `memory_recall_items` row.
pub(crate) struct RecallItem {
    pub(crate) item_kind: String,
    pub(crate) fact_ref: MemoryFactId,
    pub(crate) link_id: Option<String>,
}

pub(crate) async fn recall_items(
    db: &Db,
    recall: MemoryRecallId,
) -> Result<Vec<RecallItem>, MemoryError> {
    db.all(
        q(
            "SELECT item_kind, fact_ref, link_id FROM memory_recall_items WHERE recall_id = $1 \
           ORDER BY position",
        )
        .uuid(recall.0),
    )
    .await?
    .iter()
    .map(|row| {
        Ok(RecallItem {
            item_kind: row.text("item_kind")?,
            fact_ref: MemoryFactId(row.uuid("fact_ref")?),
            link_id: row.opt_text("link_id")?,
        })
    })
    .collect()
}

pub(crate) async fn recall_outside(
    db: &Db,
    recall: MemoryRecallId,
) -> Result<Vec<OutsideRow>, MemoryError> {
    db.all(
        q(
            "SELECT agent, repository, path, commit_id, section, page_title, as_of, answer_id, \
           more_found FROM memory_recall_wider WHERE recall_id = $1 ORDER BY position",
        )
        .uuid(recall.0),
    )
    .await?
    .iter()
    .map(|row| {
        Ok(OutsideRow {
            agent: row.text("agent")?,
            repository: row.text("repository")?,
            path: row.text("path")?,
            commit: row.text("commit_id")?,
            section: row.opt_text("section")?,
            page_title: row.opt_text("page_title")?,
            as_of: row.ts("as_of")?,
            answer_id: row.opt_uuid("answer_id")?,
            more_found: row.boolean("more_found")?,
        })
    })
    .collect()
}

// spec: ordin-desktop/components/MemoryService#SA-AMEM-08
/// The summaries still kept that were made from `origins` (SA-AMEM-08).
pub(crate) async fn summaries_of(db: &Db, origins: &[MemoryFactId]) -> Result<u32, MemoryError> {
    if origins.is_empty() {
        return Ok(0);
    }
    let mut st = q(format!(
        "SELECT COUNT(DISTINCT m.fact_ref) AS n FROM memory_made_from m \
         JOIN memory_fact_refs r ON r.id = m.fact_ref \
         WHERE r.state = 'kept' AND m.origin_ref IN ({})",
        placeholders(1, origins.len())
    ));
    for id in origins {
        st = st.uuid(id.0);
    }
    Ok(count(db, st).await? as u32)
}

pub(crate) async fn insert_made_from(
    db: &Db,
    summary: MemoryFactId,
    origins: &[MemoryFactId],
) -> Result<(), MemoryError> {
    let mut tx = db.begin().await?;
    for origin in origins {
        tx.exec(
            q(
                "INSERT INTO memory_made_from (fact_ref, origin_ref) VALUES ($1, $2) \
               ON CONFLICT DO NOTHING",
            )
            .uuid(summary.0)
            .uuid(origin.0),
        )
        .await?;
    }
    tx.commit().await
}
