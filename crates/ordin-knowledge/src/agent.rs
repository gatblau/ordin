//! The knowledge index of a hosted agent (*Internal Logic* H, a port of wrightrun's
//! KnowledgeIndex): visibility rules, question types, the ranked any-word search with thread
//! replies and one result per text, notes, entries and erasure. One storage trait, implemented on
//! SQLite with FTS5 (single shape) and on PostgreSQL (scaled shape) (SA-DB-01).

use std::collections::HashMap;
use std::sync::Arc;

use ordin_journal::Datastore;
use ordin_types::{Cursor, Page, RunId, ScopeId};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::error::KnowledgeError;
use crate::model::{
    IngestItem, ItemKind, KnowledgeItem, KnowledgeSourceConfig, MatchMode, ResultLabel,
    SearchResult, Sender, CANDIDATE_FACTOR, MAX_ENTRY_CHARS, MAX_ITEM_CHARS, MAX_QUERY_CHARS,
    MAX_RESULTS,
};
use crate::ports::{
    AgentKnowledgeDeps, ChatHistory, KnowledgeIndex, KnowledgeMetrics, KnowledgeRunContext,
    KnowledgeScopeSettings, SettingsReadError,
};

const PATTERN_MAX_CHARS: usize = 200;
const PATTERN_INPUT_CHARS: usize = 4_000;
/// The compiled size bound of one question-type pattern.
const PATTERN_SIZE_LIMIT: usize = 1 << 20;
pub(crate) const PAGE_ROWS: usize = 100;

pub(crate) struct AgentState {
    pub sources: Vec<KnowledgeSourceConfig>,
    /// (source, question type) → compiled patterns.
    pub patterns: HashMap<(String, String), Vec<Regex>>,
    pub settings: Arc<dyn KnowledgeScopeSettings>,
    pub metrics: Arc<dyn KnowledgeMetrics>,
    pub runs: Arc<dyn KnowledgeRunContext>,
    pub chat_history: Arc<dyn ChatHistory>,
    pub note_tools: bool,
}

impl AgentState {
    /// A-1: every pattern compiled (`KNOW_BAD_PATTERN`); note tools need a notes source.
    pub(crate) fn new(deps: AgentKnowledgeDeps) -> Result<AgentState, KnowledgeError> {
        let mut patterns = HashMap::new();
        for s in &deps.sources {
            for t in &s.ranking.favour_links_and_steps {
                let at = format!(
                    "knowledge[{}].ranking.favour-links-and-steps[{}]",
                    s.name, t.name
                );
                let mut compiled = Vec::with_capacity(t.patterns.len());
                for p in &t.patterns {
                    if p.chars().count() > PATTERN_MAX_CHARS {
                        return Err(KnowledgeError::BadPattern { at });
                    }
                    let re = regex::RegexBuilder::new(p)
                        .size_limit(PATTERN_SIZE_LIMIT)
                        .build()
                        .map_err(|_| KnowledgeError::BadPattern { at: at.clone() })?;
                    compiled.push(re);
                }
                patterns.insert((s.name.clone(), t.name.clone()), compiled);
            }
        }
        if deps.note_tools && !deps.sources.iter().any(|s| s.kind == ItemKind::Notes) {
            return Err(KnowledgeError::NoNotesSource);
        }
        Ok(AgentState {
            sources: deps.sources,
            patterns,
            settings: deps.settings,
            metrics: deps.metrics,
            runs: deps.runs,
            chat_history: deps.chat_history,
            note_tools: deps.note_tools,
        })
    }

    pub(crate) fn source(&self, name: &str) -> Option<&KnowledgeSourceConfig> {
        self.sources.iter().find(|s| s.name == name)
    }

    pub(crate) fn notes_sources(&self) -> impl Iterator<Item = &KnowledgeSourceConfig> {
        self.sources.iter().filter(|s| s.kind == ItemKind::Notes)
    }

    /// H-3: the first declared type whose keyword part and pattern part both hold.
    pub(crate) fn question_type(
        &self,
        source: &KnowledgeSourceConfig,
        question: Option<&str>,
    ) -> Option<String> {
        let question = question?;
        let words: Vec<String> = crate::markdown::tokens(question);
        let input: String = question.chars().take(PATTERN_INPUT_CHARS).collect();
        for t in &source.ranking.favour_links_and_steps {
            if t.keywords.is_empty() && t.patterns.is_empty() {
                continue;
            }
            let kw = |k: &String| words.contains(&k.to_lowercase());
            let keywords = t.keywords.is_empty()
                || match t.r#match {
                    MatchMode::Any => t.keywords.iter().any(kw),
                    MatchMode::All => t.keywords.iter().all(kw),
                };
            let compiled = self
                .patterns
                .get(&(source.name.clone(), t.name.clone()))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let pattern = compiled.is_empty()
                || match t.r#match {
                    MatchMode::Any => compiled.iter().any(|r| r.is_match(&input)),
                    MatchMode::All => compiled.iter().all(|r| r.is_match(&input)),
                };
            if keywords && pattern {
                return Some(t.name.clone());
            }
        }
        None
    }
}

/// H-3: `http://` or `https://` followed by a character that is not white space.
pub(crate) fn has_link(text: &str) -> bool {
    for marker in ["http://", "https://"] {
        let mut rest = text;
        while let Some(i) = rest.find(marker) {
            let after = &rest[i + marker.len()..];
            if after.chars().next().is_some_and(|c| !c.is_whitespace()) {
                return true;
            }
            rest = after;
        }
    }
    false
}

/// H-3: at least two lines beginning, after optional white space, with 1–2 digits and `.` or `)`,
/// or with `-`, `*`, `•`, then white space and a further character.
pub(crate) fn has_steps(text: &str) -> bool {
    let mut n = 0;
    for line in text.lines() {
        let l = line.trim_start();
        let rest = {
            let digits = l.chars().take_while(char::is_ascii_digit).count();
            if (1..=2).contains(&digits) {
                let after = &l[digits..];
                after.strip_prefix('.').or_else(|| after.strip_prefix(')'))
            } else {
                l.strip_prefix('-')
                    .or_else(|| l.strip_prefix('*'))
                    .or_else(|| l.strip_prefix('•'))
            }
        };
        if let Some(r) = rest {
            let mut cs = r.chars();
            if cs.next().is_some_and(char::is_whitespace) && cs.any(|c| !c.is_whitespace()) {
                n += 1;
                if n >= 2 {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The visibility values of one source in one scope (H-1).
#[derive(Debug, Clone, Default)]
pub(crate) struct Vis {
    pub scope: Option<ScopeId>,
    pub include_own: bool,
    pub hidden: Vec<String>,
}

/// One item prepared for K1.
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    pub item_id: uuid::Uuid,
    pub origin_id: String,
    pub origin_time: chrono::DateTime<chrono::Utc>,
    pub sender: String,
    pub sender_name: Option<String>,
    pub from_program: bool,
    pub own: bool,
    pub thread: Option<String>,
    pub body: String,
    pub body_hash: String,
    pub has_link: bool,
    pub has_steps: bool,
}

/// A candidate row of K2/K3.
#[derive(Debug, Clone)]
pub(crate) struct Ranked {
    pub item: KnowledgeItem,
    pub rank: f64,
    pub body_hash: String,
}

/// The statements K1–K19 on one engine.
#[async_trait::async_trait]
pub(crate) trait ItemStore: Send + Sync {
    async fn upsert(
        &self,
        source: &str,
        scope: ScopeId,
        connection: &str,
        language: &str,
        items: &[Prepared],
    ) -> Result<u64, KnowledgeError>;
    async fn search(
        &self,
        source: &str,
        language: &str,
        vis: &Vis,
        query: &str,
        boost: bool,
        candidates: u32,
    ) -> Result<Vec<Ranked>, KnowledgeError>;
    async fn replies(
        &self,
        source: &str,
        language: &str,
        vis: &Vis,
        query: &str,
        roots: &[String],
    ) -> Result<Vec<Ranked>, KnowledgeError>;
    async fn recent(
        &self,
        source: &str,
        vis: &Vis,
        limit: u32,
    ) -> Result<Vec<KnowledgeItem>, KnowledgeError>;
    async fn open(&self, item_id: uuid::Uuid) -> Result<Option<KnowledgeItem>, KnowledgeError>;
    async fn senders(&self, source: &str, scope: ScopeId) -> Result<Vec<Sender>, KnowledgeError>;
    #[allow(clippy::too_many_arguments)]
    async fn note_insert(
        &self,
        item_id: uuid::Uuid,
        source: &str,
        scope: Option<ScopeId>,
        body: &str,
        language: &str,
        run_id: RunId,
        max_notes: u32,
    ) -> Result<bool, KnowledgeError>;
    async fn note_update(
        &self,
        note_id: uuid::Uuid,
        scope: Option<ScopeId>,
        body: &str,
        run_id: RunId,
    ) -> Result<u64, KnowledgeError>;
    async fn note_retire(
        &self,
        note_id: uuid::Uuid,
        scope: Option<ScopeId>,
    ) -> Result<u64, KnowledgeError>;
    async fn page(
        &self,
        kind: ItemKind,
        scope: Option<ScopeId>,
        after: Option<(String, String)>,
        limit: usize,
    ) -> Result<Vec<(KnowledgeItem, String)>, KnowledgeError>;
    async fn entry_insert(
        &self,
        item_id: uuid::Uuid,
        source: &str,
        scope: ScopeId,
        body: &str,
        language: &str,
        by: &str,
    ) -> Result<(), KnowledgeError>;
    async fn entry_verify(&self, entry_id: uuid::Uuid, by: &str) -> Result<u64, KnowledgeError>;
    async fn entry_remove(&self, entry_id: uuid::Uuid) -> Result<u64, KnowledgeError>;
    async fn delete_older_than(
        &self,
        kind: ItemKind,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: u32,
    ) -> Result<u64, KnowledgeError>;
    async fn delete_by_sender(&self, connection: &str, sender: &str)
        -> Result<u64, KnowledgeError>;
    async fn delete_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError>;
    async fn count_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError>;
    async fn count_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError>;
}

pub(crate) struct Index {
    pub state: Arc<AgentState>,
    pub store: Box<dyn ItemStore>,
}

impl Index {
    pub(crate) fn new(state: Arc<AgentState>, datastore: Datastore) -> Index {
        let store: Box<dyn ItemStore> = match datastore {
            Datastore::Sqlite(pool) => Box::new(crate::agent_sqlite::SqliteItems { pool }),
            Datastore::Postgres(pool) => Box::new(crate::agent_pg::PgItems { pool }),
        };
        Index { state, store }
    }

    /// H-1 rules 4 and 5: the settings read at every call; nothing is cached.
    pub(crate) async fn vis(
        &self,
        source: &KnowledgeSourceConfig,
        scope: Option<ScopeId>,
    ) -> Result<Vis, KnowledgeError> {
        let mut vis = Vis {
            scope,
            ..Vis::default()
        };
        let Some(scope) = scope else {
            return Ok(vis);
        };
        if source.kind != ItemKind::ChatHistory {
            return Ok(vis);
        }
        let s = &self.state.settings;
        if let Some(name) = &source.hidden_senders_setting {
            vis.hidden = match s.selection(scope, name).await {
                Ok(v) => v,
                Err(SettingsReadError::Unavailable) => return Err(KnowledgeError::DbUnavailable),
                Err(SettingsReadError::Unknown) => Vec::new(),
            };
        }
        if let Some(name) = &source.own_messages_setting {
            vis.include_own = match s.flag(scope, name).await {
                Ok(v) => v,
                Err(SettingsReadError::Unavailable) => return Err(KnowledgeError::DbUnavailable),
                Err(SettingsReadError::Unknown) => false,
            };
        }
        Ok(vis)
    }

    /// H-5 for a list of sources (H-9 for one, with no notes source added).
    async fn ranked_search(
        &self,
        sources: &[String],
        scope: Option<ScopeId>,
        query: &str,
        question: Option<&str>,
        limit: u32,
        operation_log: &'static str,
    ) -> Result<Vec<SearchResult>, KnowledgeError> {
        let mut names: Vec<&str> = Vec::new();
        for s in sources {
            if self.state.source(s).is_none() {
                return Err(KnowledgeError::UnknownSource);
            }
            if !names.contains(&s.as_str()) {
                names.push(s);
            }
        }
        let limit = limit.clamp(1, MAX_RESULTS);
        let query = cut(query, MAX_QUERY_CHARS);
        let candidates = (limit * CANDIDATE_FACTOR).min(150);
        let mut all: Vec<(Ranked, &KnowledgeSourceConfig)> = Vec::new();
        let mut boosted_any = false;
        let mut type_name: Option<String> = None;
        let mut vis_of: HashMap<&str, Vis> = HashMap::new();
        for name in &names {
            let src = self
                .state
                .source(name)
                .ok_or(KnowledgeError::UnknownSource)?;
            let vis = self.vis(src, scope).await?;
            let qt = if src.kind == ItemKind::ChatHistory {
                self.state.question_type(src, question)
            } else {
                None
            };
            let boost = qt.is_some();
            boosted_any |= boost;
            if type_name.is_none() {
                type_name = qt;
            }
            let rows = self
                .store
                .search(name, &src.language, &vis, &query, boost, candidates)
                .await?;
            self.state.metrics.query(name, "search");
            vis_of.insert(name, vis);
            all.extend(rows.into_iter().map(|r| (r, src)));
        }
        all.sort_by(|a, b| {
            b.0.rank
                .partial_cmp(&a.0.rank)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.0.item.origin_time.cmp(&a.0.item.origin_time))
                .then_with(|| a.0.item.item_id.cmp(&b.0.item.item_id))
        });
        let mut results: Vec<(SearchResult, String, &str)> = all
            .into_iter()
            .map(|(r, src)| {
                (
                    SearchResult {
                        label: ResultLabel::of(r.item.kind),
                        rank: r.rank,
                        reply: None,
                        item: r.item,
                    },
                    r.body_hash,
                    src.name.as_str(),
                )
            })
            .collect();
        // Thread-aware results (wrightrun ADR-034).
        for name in &names {
            let src = self
                .state
                .source(name)
                .ok_or(KnowledgeError::UnknownSource)?;
            if !src.ranking.thread_replies {
                continue;
            }
            let roots: Vec<String> = results
                .iter()
                .filter(|(r, _, s)| {
                    *s == *name && r.item.thread.is_some() && r.item.thread == r.item.origin_id
                })
                .filter_map(|(r, _, _)| r.item.origin_id.clone())
                .collect();
            if roots.is_empty() {
                continue;
            }
            let vis = vis_of.get(name).cloned().unwrap_or_default();
            let replies = self
                .store
                .replies(name, &src.language, &vis, &query, &roots)
                .await?;
            let mut attached: Vec<uuid::Uuid> = Vec::new();
            for (r, _, s) in results.iter_mut() {
                if *s != *name {
                    continue;
                }
                if let Some(rep) = replies
                    .iter()
                    .find(|x| x.item.thread.is_some() && x.item.thread == r.item.origin_id)
                {
                    attached.push(rep.item.item_id);
                    r.reply = Some(rep.item.clone());
                }
            }
            results.retain(|(r, _, _)| !attached.contains(&r.item.item_id));
        }
        // One result for each text.
        let mut seen: Vec<(String, String)> = Vec::new();
        results.retain(|(_, hash, s)| {
            let one = self
                .state
                .source(s)
                .is_some_and(|src| src.ranking.one_per_text);
            if !one {
                return true;
            }
            let key = ((*s).to_owned(), hash.clone());
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
        results.truncate(limit as usize);
        let out: Vec<SearchResult> = results.into_iter().map(|(r, _, _)| r).collect();
        let scope_text = scope
            .map(|s| s.to_string())
            .unwrap_or_else(|| "agent-wide".to_owned());
        if operation_log == "knowledge_lookup" {
            tracing::info!(
                component = "knowledge",
                source = names.first().copied().unwrap_or(""),
                scope_id = scope_text,
                results = out.len(),
                "knowledge_lookup"
            );
        } else {
            tracing::info!(
                component = "knowledge",
                sources = names.join(","),
                scope_id = scope_text,
                results = out.len(),
                boosted = boosted_any,
                question_type = type_name.as_deref().unwrap_or(""),
                "knowledge_search"
            );
        }
        Ok(out)
    }

    fn note_source(&self, source: &str) -> Result<&KnowledgeSourceConfig, KnowledgeError> {
        self.state
            .source(source)
            .filter(|s| s.kind == ItemKind::Notes)
            .ok_or(KnowledgeError::UnknownSource)
    }

    /// The notes source a note id belongs to, for the length rule of `note_replace`.
    async fn note_source_of(
        &self,
        id: uuid::Uuid,
    ) -> Result<Option<&KnowledgeSourceConfig>, KnowledgeError> {
        Ok(self
            .store
            .open(id)
            .await?
            .filter(|i| i.kind == ItemKind::Notes)
            .and_then(|i| self.state.source(&i.source)))
    }
}

fn parse_uuid(raw: &str) -> Option<uuid::Uuid> {
    uuid::Uuid::parse_str(raw).ok()
}

#[async_trait::async_trait]
impl KnowledgeIndex for Index {
    /// H-4.
    async fn upsert(
        &self,
        source: &str,
        scope: ScopeId,
        items: &[IngestItem],
    ) -> Result<u64, KnowledgeError> {
        let src = self
            .state
            .source(source)
            .filter(|s| s.kind == ItemKind::ChatHistory)
            .ok_or(KnowledgeError::UnknownSource)?;
        let connection = src.connection.clone().unwrap_or_default();
        // Last item wins per origin_id.
        let mut by_origin: Vec<&IngestItem> = Vec::new();
        for i in items {
            if let Some(pos) = by_origin.iter().position(|x| x.origin_id == i.origin_id) {
                by_origin.remove(pos);
            }
            by_origin.push(i);
        }
        let prepared: Vec<Prepared> = by_origin
            .into_iter()
            .filter(|i| !i.text.is_empty())
            .map(|i| {
                let body = cut(&i.text, MAX_ITEM_CHARS);
                Prepared {
                    item_id: uuid::Uuid::now_v7(),
                    origin_id: i.origin_id.clone(),
                    origin_time: i.origin_time,
                    sender: i.sender.clone(),
                    sender_name: i.sender_name.clone(),
                    from_program: i.from_program,
                    own: i.own,
                    thread: i.thread.clone(),
                    body_hash: sha256_hex(&body),
                    has_link: has_link(&body),
                    has_steps: has_steps(&body),
                    body,
                }
            })
            .collect();
        if prepared.is_empty() {
            return Ok(0);
        }
        self.store
            .upsert(source, scope, &connection, &src.language, &prepared)
            .await
    }

    /// H-5.
    async fn search(
        &self,
        sources: &[String],
        scope: Option<ScopeId>,
        query: &str,
        question: Option<&str>,
        limit: u32,
    ) -> Result<Vec<SearchResult>, KnowledgeError> {
        self.ranked_search(sources, scope, query, question, limit, "knowledge_search")
            .await
    }

    /// H-6 / K4.
    async fn recent(
        &self,
        source: &str,
        scope: Option<ScopeId>,
        limit: u32,
    ) -> Result<Vec<KnowledgeItem>, KnowledgeError> {
        let src = self
            .state
            .source(source)
            .ok_or(KnowledgeError::UnknownSource)?;
        let vis = self.vis(src, scope).await?;
        let rows = self
            .store
            .recent(source, &vis, limit.clamp(1, MAX_RESULTS))
            .await?;
        self.state.metrics.query(source, "recent");
        Ok(rows)
    }

    /// H-9 (ADR-107): one source, `query = question`, the question-type boost applied.
    async fn lookup(
        &self,
        source: &str,
        scope: Option<ScopeId>,
        question: &str,
        limit: u32,
    ) -> Result<Vec<SearchResult>, KnowledgeError> {
        let limit = limit.clamp(1, 20);
        self.ranked_search(
            &[source.to_owned()],
            scope,
            question,
            Some(question),
            limit,
            "knowledge_lookup",
        )
        .await
    }

    /// H-6 / K5: rule 1 only.
    async fn open(
        &self,
        item_id: &str,
        scope: Option<ScopeId>,
    ) -> Result<Option<KnowledgeItem>, KnowledgeError> {
        let Some(id) = parse_uuid(item_id) else {
            return Ok(None);
        };
        let Some(item) = self.store.open(id).await? else {
            return Ok(None);
        };
        self.state.metrics.query(&item.source, "open");
        let visible =
            item.scope_id == scope || (item.kind == ItemKind::Notes && item.scope_id.is_none());
        Ok(visible.then_some(item))
    }

    async fn senders(&self, source: &str, scope: ScopeId) -> Result<Vec<Sender>, KnowledgeError> {
        self.state
            .source(source)
            .ok_or(KnowledgeError::UnknownSource)?;
        self.store.senders(source, scope).await
    }

    async fn note_write(
        &self,
        source: &str,
        scope: Option<ScopeId>,
        text: &str,
        run_id: RunId,
    ) -> Result<uuid::Uuid, KnowledgeError> {
        let src = self.note_source(source)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(KnowledgeError::NoteEmpty);
        }
        let max_chars = src.max_note_chars.unwrap_or(u32::MAX);
        let len = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
        if len > max_chars {
            return Err(KnowledgeError::NoteTooLong {
                max: max_chars,
                len,
            });
        }
        let max_notes = src.max_notes_per_scope.unwrap_or(u32::MAX);
        let id = uuid::Uuid::now_v7();
        if self
            .store
            .note_insert(id, source, scope, text, &src.language, run_id, max_notes)
            .await?
        {
            Ok(id)
        } else {
            Err(KnowledgeError::NoteLimit { max: max_notes })
        }
    }

    async fn note_replace(
        &self,
        note_id: &str,
        scope: Option<ScopeId>,
        text: &str,
        run_id: RunId,
    ) -> Result<(), KnowledgeError> {
        let id = parse_uuid(note_id).ok_or(KnowledgeError::NoteNotFound)?;
        let text = text.trim();
        if text.is_empty() {
            return Err(KnowledgeError::NoteEmpty);
        }
        if let Some(src) = self.note_source_of(id).await? {
            let max = src.max_note_chars.unwrap_or(u32::MAX);
            let len = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
            if len > max {
                return Err(KnowledgeError::NoteTooLong { max, len });
            }
        }
        if self.store.note_update(id, scope, text, run_id).await? == 0 {
            return Err(KnowledgeError::NoteNotFound);
        }
        Ok(())
    }

    async fn note_retire(
        &self,
        note_id: &str,
        scope: Option<ScopeId>,
    ) -> Result<(), KnowledgeError> {
        let id = parse_uuid(note_id).ok_or(KnowledgeError::NoteNotFound)?;
        if self.store.note_retire(id, scope).await? == 0 {
            return Err(KnowledgeError::NoteNotFound);
        }
        Ok(())
    }

    /// 6a: newest first, 100 a page.
    async fn notes(
        &self,
        scope: Option<ScopeId>,
        cursor: Option<&str>,
    ) -> Result<Page<KnowledgeItem>, KnowledgeError> {
        self.page_of(ItemKind::Notes, scope, cursor).await
    }

    async fn entry_add(
        &self,
        source: &str,
        scope: ScopeId,
        text: &str,
        by: &str,
    ) -> Result<uuid::Uuid, KnowledgeError> {
        let src = self
            .state
            .source(source)
            .filter(|s| s.kind == ItemKind::Entries)
            .ok_or(KnowledgeError::UnknownSource)?;
        let n = text.chars().count();
        if n == 0 || n > MAX_ENTRY_CHARS {
            return Err(KnowledgeError::EntryInvalid);
        }
        let id = uuid::Uuid::now_v7();
        self.store
            .entry_insert(id, source, scope, text, &src.language, by)
            .await?;
        Ok(id)
    }

    async fn entry_verify(&self, entry_id: &str, by: &str) -> Result<(), KnowledgeError> {
        let id = parse_uuid(entry_id).ok_or(KnowledgeError::EntryNotFound)?;
        if self.store.entry_verify(id, by).await? == 0 {
            return Err(KnowledgeError::EntryNotFound);
        }
        Ok(())
    }

    async fn entry_remove(&self, entry_id: &str) -> Result<(), KnowledgeError> {
        let id = parse_uuid(entry_id).ok_or(KnowledgeError::EntryNotFound)?;
        if self.store.entry_remove(id).await? == 0 {
            return Err(KnowledgeError::EntryNotFound);
        }
        Ok(())
    }

    async fn entries(
        &self,
        scope: ScopeId,
        cursor: Option<&str>,
    ) -> Result<Page<KnowledgeItem>, KnowledgeError> {
        self.page_of(ItemKind::Entries, Some(scope), cursor).await
    }

    async fn delete_older_than(
        &self,
        kind: ItemKind,
        cutoff: chrono::DateTime<chrono::Utc>,
        limit: u32,
    ) -> Result<u64, KnowledgeError> {
        self.store.delete_older_than(kind, cutoff, limit).await
    }

    async fn delete_by_sender(
        &self,
        connection: &str,
        sender: &str,
    ) -> Result<u64, KnowledgeError> {
        self.store.delete_by_sender(connection, sender).await
    }

    async fn delete_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError> {
        self.store.delete_scope(scope).await
    }

    async fn count_by_sender(&self, connection: &str, sender: &str) -> Result<u64, KnowledgeError> {
        self.store.count_by_sender(connection, sender).await
    }

    async fn count_scope(&self, scope: Option<ScopeId>) -> Result<u64, KnowledgeError> {
        self.store.count_scope(scope).await
    }
}

impl Index {
    async fn page_of(
        &self,
        kind: ItemKind,
        scope: Option<ScopeId>,
        cursor: Option<&str>,
    ) -> Result<Page<KnowledgeItem>, KnowledgeError> {
        let after = match cursor {
            Some(raw) => {
                let c: Cursor = serde_json::from_value(serde_json::Value::String(raw.to_owned()))
                    .map_err(|_| KnowledgeError::BadCursor)?;
                Some(
                    c.decode::<(String, String)>()
                        .map_err(|_| KnowledgeError::BadCursor)?,
                )
            }
            None => None,
        };
        let mut rows = self.store.page(kind, scope, after, PAGE_ROWS + 1).await?;
        let next_cursor = if rows.len() > PAGE_ROWS {
            rows.truncate(PAGE_ROWS);
            rows.last()
                .map(|(i, created)| Cursor::encode(&(created.clone(), i.item_id.to_string())))
        } else {
            None
        };
        Ok(Page {
            items: rows.into_iter().map(|(i, _)| i).collect(),
            next_cursor,
        })
    }
}

/// The delete of `delete_older_than` with the kind as the operator command's string input:
/// a kind outside the three is `KNOW_UNKNOWN_KIND` (H-6).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn kind_from_text(raw: &str) -> Result<ItemKind, KnowledgeError> {
    ItemKind::from_text(raw).ok_or(KnowledgeError::UnknownKind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_steps() {
        assert!(has_link("see https://x.example/a"));
        assert!(!has_link("https:// only"));
        assert!(has_steps("1. one\n2) two"));
        assert!(has_steps("- a\n* b"));
        assert!(!has_steps("1. only one"));
    }
}
