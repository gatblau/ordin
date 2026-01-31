//! The MCP server `knowledge` (*Internal Logic* G for the desktop and the personal server, H-7 for
//! the agent profile), with the result additions of ADR-104 (§S, §T), ADR-110 (§V) and ADR-112.

use std::sync::Arc;

use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolCaller,
    ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolSpec,
};
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgePageState};
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::{KnowledgePageId, RunId};
use serde_json::{json, Map, Value};

use crate::agent::{cut, Index};
use crate::error::KnowledgeError;
use crate::markdown::snippet;
use crate::model::{
    ExposedOperation, ItemKind, KnowledgeItem, KnowledgeSourceConfig, ResultLabel, SearchResult,
    MAX_RESULTS, OPEN_TEXT_CHARS, RESULT_TEXT_CHARS,
};
use crate::okf;
use crate::ports::{KnowledgeIndex, SettingsReadError, SourceReadCheck};
use crate::read::ReadRow;
use crate::search::{Candidate, ToolFilter};
use crate::service::{KnowledgeService, MAX_TOOL_LINKS};
use crate::store::{self, PageRow, StoredSource};

const SERVER: &str = "knowledge";
const SEARCH_PAGES: &str = "knowledge_search_pages";
const READ_PAGE: &str = "knowledge_read_page";
const SNIPPET_CHARS: usize = 300;
const SEARCH_REASON: &str =
    "Give a query or at least one of state, written_by, tag; query 1 to 500 characters; limit 1 to 20.";
const READ_REASON: &str = "page_id must be a page id a search returned.";
const NOT_AVAILABLE: &str = "No page with that id is available in this workspace.";
const DB_DOWN: &str = "The knowledge pages cannot be read at the moment.";
const AGENT_DOWN: &str = "The knowledge index cannot be read at the moment (KNOW_DB_UNAVAILABLE).";
const NOTE_DOWN: &str = "The knowledge index cannot be reached at the moment, and whether the note was changed is not known (KNOW_DB_UNAVAILABLE).";

pub(crate) struct KnowledgeTools {
    pub svc: Arc<KnowledgeService>,
}

fn server() -> ServerName {
    ServerName::new(SERVER).unwrap_or_else(|_| unreachable_server())
}

#[allow(clippy::panic)]
fn unreachable_server() -> ServerName {
    // "knowledge" matches the server-name pattern; this branch cannot be taken.
    ServerName::new("k").unwrap_or_else(|_| panic!("server name"))
}

fn search_schema() -> Value {
    json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":500},
        "state":{"enum":["current","may_be_out_of_date"]},"written_by":{"enum":["ordin","person"]},
        "tag":{"type":"string","minLength":1,"maxLength":64},"limit":{"type":"integer","minimum":1,"maximum":20}},
        "additionalProperties":false,"minProperties":1})
}

fn read_schema() -> Value {
    json!({"type":"object","properties":{"page_id":{"type":"string","format":"uuid"}},"required":["page_id"],"additionalProperties":false})
}

fn tool_name(op: &str, source: &str) -> String {
    format!("{op}_{}", source.replace('-', "_"))
}

fn db_failure() -> ToolFailure {
    ToolFailure {
        code: "KNOW_DB_UNAVAILABLE".to_owned(),
        message: DB_DOWN.to_owned(),
        retryable: true,
    }
}

fn not_found() -> ToolFailure {
    ToolFailure {
        code: "KNOW_PAGE_NOT_FOUND".to_owned(),
        message: NOT_AVAILABLE.to_owned(),
        retryable: false,
    }
}

fn text_out(text: String) -> ToolOutput {
    ToolOutput {
        text,
        structured: None,
    }
}

fn json_out(value: Value) -> ToolOutput {
    ToolOutput {
        text: value.to_string(),
        structured: Some(value),
    }
}

/// G-2: the desktop tools' fixed-code argument check.
fn check_search(args: &Map<String, Value>) -> bool {
    let mut any = false;
    for (k, v) in args {
        let ok = match k.as_str() {
            "query" => v
                .as_str()
                .is_some_and(|s| (1..=500).contains(&s.chars().count())),
            "state" => matches!(v.as_str(), Some("current" | "may_be_out_of_date")),
            "written_by" => matches!(v.as_str(), Some("ordin" | "person")),
            "tag" => v
                .as_str()
                .is_some_and(|s| (1..=64).contains(&s.chars().count())),
            "limit" => v.as_u64().is_some_and(|n| (1..=20).contains(&n)) && v.is_u64(),
            _ => false,
        };
        if !ok {
            return false;
        }
        any |= k != "limit";
    }
    any
}

fn parse_page_id(raw: &str) -> Option<KnowledgePageId> {
    uuid::Uuid::parse_str(raw).ok().map(KnowledgePageId)
}

fn tier_word(t: PageTier) -> &'static str {
    store::tier_text(t)
}

fn status_value(s: PageStatus) -> Option<&'static str> {
    match s {
        PageStatus::Draft => Some("draft"),
        PageStatus::Deprecated => Some("deprecated"),
        PageStatus::Stable => None,
    }
}

/// The ADR-112 fields of a result.
fn okf_fields(
    o: &mut Map<String, Value>,
    page_type: &Option<String>,
    description: &Option<String>,
    tier: PageTier,
    status: PageStatus,
    stale_after: &Option<String>,
) {
    if let Some(t) = page_type {
        o.insert("type".into(), json!(t));
    }
    if let Some(d) = description {
        o.insert("description".into(), json!(d));
    }
    o.insert("trust".into(), json!(tier_word(tier)));
    if let Some(s) = status_value(status) {
        o.insert("status".into(), json!(s));
    }
    let past = stale_after
        .as_deref()
        .and_then(okf::parse_stale_after)
        .is_some_and(|s| s <= ordin_types::time::Timestamp::now());
    o.insert("past_stale_after".into(), json!(past));
}

impl KnowledgeTools {
    fn desktop_specs() -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: SEARCH_PAGES.to_owned(),
                description: "Search this workspace's knowledge pages by words, state, author or tag; each result names a page id.".to_owned(),
                input_schema: search_schema(),
                argument_form: r#"{"query": "<words>", "state": "current" | "may_be_out_of_date", "written_by": "ordin" | "person", "tag": "<tag>", "limit": 1-20} (at least one of query, state, written_by, tag)"#.to_owned(),
                sandbox_callable: true,
                plain: "Can search this workspace's knowledge pages.".to_owned(),
            },
            ToolSpec {
                name: READ_PAGE.to_owned(),
                description: "Read one knowledge page with its sources and links; says if it may be out of date.".to_owned(),
                input_schema: read_schema(),
                argument_form: r#"{"page_id": "<a page id a search returned>"}"#.to_owned(),
                sandbox_callable: true,
                plain: "Can read a knowledge page and say whether it may be out of date.".to_owned(),
            },
        ]
    }

    fn agent_specs(&self) -> Vec<ToolSpec> {
        let Some(agent) = &self.svc.agent else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for s in &agent.sources {
            for (op, exposed) in [
                ("search", ExposedOperation::Search),
                ("recent", ExposedOperation::Recent),
                ("open", ExposedOperation::Open),
            ] {
                if !s.expose.contains(&exposed) {
                    continue;
                }
                let (schema, description, form) = match op {
                    "search" => (
                        json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":500,"description":"Words to look for. An item matches when it shares any of them."}},"required":["query"],"additionalProperties":false}),
                        format!("Search the knowledge source \"{}\". Returns the best-matching items of the place this conversation is in, with the agent's own notes. Each result names an item identifier.", s.name),
                        r#"{"query": "<words to look for>"}"#,
                    ),
                    "recent" => (
                        json!({"type":"object","properties":{},"additionalProperties":false}),
                        format!("List the most recent items of the knowledge source \"{}\" in the place this conversation is in.", s.name),
                        "{}",
                    ),
                    _ => (
                        json!({"type":"object","properties":{"item_id":{"type":"string","description":"An item identifier a search or a listing returned."}},"required":["item_id"],"additionalProperties":false}),
                        format!("Open one item of the knowledge source \"{}\" and read its full text. Open an item before you cite it as a source.", s.name),
                        r#"{"item_id": "<an item identifier>"}"#,
                    ),
                };
                out.push(ToolSpec {
                    name: tool_name(op, &s.name),
                    description,
                    input_schema: schema,
                    argument_form: form.to_owned(),
                    sandbox_callable: true,
                    plain: format!("Can search and read {}.", s.name),
                });
            }
        }
        if let Some(notes) = self.notes_source() {
            let max = notes.max_note_chars.unwrap_or(0);
            out.push(ToolSpec {
                name: "note_write".to_owned(),
                description: format!("Keep a note for later conversations in this place. A note holds at most {max} characters."),
                input_schema: json!({"type":"object","properties":{"text":{"type":"string","minLength":1}},"required":["text"],"additionalProperties":false}),
                argument_form: r#"{"text": "<the note>"}"#.to_owned(),
                sandbox_callable: false,
                plain: "Can keep notes for later conversations.".to_owned(),
            });
            out.push(ToolSpec {
                name: "note_replace".to_owned(),
                description: "Replace the text of one of your notes of this place.".to_owned(),
                input_schema: json!({"type":"object","properties":{"note_id":{"type":"string"},"text":{"type":"string","minLength":1}},"required":["note_id","text"],"additionalProperties":false}),
                argument_form: r#"{"note_id": "<a note id>", "text": "<the new text>"}"#.to_owned(),
                sandbox_callable: false,
                plain: "Can keep notes for later conversations.".to_owned(),
            });
            out.push(ToolSpec {
                name: "note_retire".to_owned(),
                description: "Retire one of your notes of this place. A retired note is no longer returned.".to_owned(),
                input_schema: json!({"type":"object","properties":{"note_id":{"type":"string"}},"required":["note_id"],"additionalProperties":false}),
                argument_form: r#"{"note_id": "<a note id>"}"#.to_owned(),
                sandbox_callable: false,
                plain: "Can keep notes for later conversations.".to_owned(),
            });
        }
        out
    }

    fn notes_source(&self) -> Option<&KnowledgeSourceConfig> {
        let agent = self.svc.agent.as_ref()?;
        if !agent.note_tools {
            return None;
        }
        agent.sources.iter().find(|s| s.kind == ItemKind::Notes)
    }

    // ------------------------------------------------------------------ desktop (G)

    /// G-3: a candidate is visible when every source passes the Rules Engine (fail closed).
    async fn visible(
        &self,
        ctx: &ToolCallContext,
        page_id: KnowledgePageId,
        sources: &[StoredSource],
        read: bool,
    ) -> bool {
        for s in sources {
            if read && s.origin == "place" {
                return false;
            }
            let check = SourceReadCheck {
                workspace_id: ctx.workspace_id,
                caller: ctx.caller,
                agent_instance_id: ctx.agent_instance_id,
                origin: s.origin.clone(),
                location: s.location.clone(),
            };
            match self.svc.rules.may_read_source(&check).await {
                Ok(true) => {}
                Ok(false) => return false,
                Err(_) => {
                    tracing::warn!(
                        component = "knowledge",
                        page_id = page_id.to_string().as_str(),
                        "knowledge_rules_unavailable"
                    );
                    return false;
                }
            }
        }
        true
    }

    async fn folder_fields(&self, o: &mut Map<String, Value>, row: &ReadRow) {
        if let Some(s) = self.svc.read_summary_of(row).await {
            if let Some(f) = s.folder {
                o.insert("folder".into(), json!(f));
            }
            if let Some(d) = s.folder_description {
                o.insert("folder_description".into(), json!(d));
            }
            o.insert(
                "set_by_company".into(),
                json!(s.set_by_company.unwrap_or(false)),
            );
        }
    }

    async fn search_pages(
        &self,
        ctx: &ToolCallContext,
        args: &Map<String, Value>,
    ) -> Result<ToolOutput, ToolFailure> {
        let filter = ToolFilter {
            query: args.get("query").and_then(Value::as_str).map(str::to_owned),
            state: match args.get("state").and_then(Value::as_str) {
                Some("current") => Some(KnowledgePageState::Current),
                Some("may_be_out_of_date") => Some(KnowledgePageState::MayBeOutOfDate),
                _ => None,
            },
            written_by: match args.get("written_by").and_then(Value::as_str) {
                Some("ordin") => Some(KnowledgeAuthor::Ordin),
                Some("person") => Some(KnowledgeAuthor::Person),
                _ => None,
            },
            tag: args.get("tag").and_then(Value::as_str).map(str::to_owned),
        };
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map_or(5, |n| n.clamp(1, 20)) as u32;
        let candidates = self
            .svc
            .tool_candidates(ctx.workspace_id, &filter, limit)
            .await
            .map_err(|_| db_failure())?;
        let query = filter.query.clone().unwrap_or_default();
        let mut results = Vec::new();
        let mut withheld = 0u32;
        for c in candidates {
            let (id, sources, read) = match &c {
                Candidate::Own(r) => (r.id, r.sources.clone(), false),
                Candidate::Read(r) => (r.page_id, r.sources.clone(), true),
            };
            if !self.visible(ctx, id, &sources, read).await {
                withheld += 1;
                continue;
            }
            if results.len() >= limit as usize {
                continue;
            }
            let mut o = Map::new();
            match &c {
                Candidate::Own(r) => {
                    o.insert("page_id".into(), json!(r.id));
                    o.insert("title".into(), json!(r.title));
                    okf_fields(
                        &mut o,
                        &r.page_type,
                        &r.description,
                        r.okf_tier,
                        r.okf_status,
                        &r.stale_after,
                    );
                    o.insert("state".into(), json!(store::state_text(r.state)));
                    o.insert("tags".into(), json!(r.tags));
                    o.insert(
                        "snippet".into(),
                        json!(snippet(&r.body, &query, SNIPPET_CHARS)),
                    );
                }
                Candidate::Read(r) => {
                    o.insert("page_id".into(), json!(r.page_id));
                    o.insert("title".into(), json!(r.title));
                    okf_fields(
                        &mut o,
                        &r.page_type,
                        &r.description,
                        r.okf_tier,
                        r.okf_status,
                        &r.stale_after,
                    );
                    o.insert("state".into(), json!("current"));
                    self.folder_fields(&mut o, r).await;
                    o.insert("tags".into(), json!(r.tags));
                    o.insert(
                        "snippet".into(),
                        json!(snippet(&r.body, &query, SNIPPET_CHARS)),
                    );
                }
            }
            results.push(Value::Object(o));
        }
        self.svc
            .journal(
                "searched",
                Some(ctx.workspace_id),
                Some(ctx.run_id),
                "A session searched the knowledge pages.",
                json!({ "run_id": ctx.run_id, "results": results.len(), "withheld": withheld }),
            )
            .await;
        Ok(json_out(
            json!({ "results": results, "withheld": withheld }),
        ))
    }

    /// T-2: a session's link fields; a withheld page is never named.
    async fn tool_links(
        &self,
        ctx: &ToolCallContext,
        row: &PageRow,
    ) -> Result<Map<String, Value>, KnowledgeError> {
        let mut conn = self.svc.conn().await?;
        let (out, ins) = crate::links::person_links(&mut conn, row).await?;
        let mut withheld = 0u32;
        let mut links_out = Vec::new();
        let mut out_more = 0u32;
        for l in out {
            let entry = match &l.target {
                Some(t) => {
                    let target = store::page(&mut conn, t.page_id).await?;
                    let ok = match &target {
                        Some(tr) => self.visible(ctx, tr.id, &tr.sources, false).await,
                        None => false,
                    };
                    if !ok {
                        withheld += 1;
                        continue;
                    }
                    let mut o = Map::new();
                    o.insert("page_id".into(), json!(t.page_id));
                    o.insert("title".into(), json!(t.title));
                    if let Some(s) = &l.section {
                        o.insert("section".into(), json!(s));
                    }
                    Value::Object(o)
                }
                None => json!({ "page_id": null, "name": l.name }),
            };
            if links_out.len() >= MAX_TOOL_LINKS {
                out_more += 1;
                continue;
            }
            links_out.push(entry);
        }
        let mut links_in = Vec::new();
        let mut in_more = 0u32;
        for p in ins {
            let src = store::page(&mut conn, p.page_id).await?;
            let ok = match &src {
                Some(s) => self.visible(ctx, s.id, &s.sources, false).await,
                None => false,
            };
            if !ok {
                withheld += 1;
                continue;
            }
            if links_in.len() >= MAX_TOOL_LINKS {
                in_more += 1;
                continue;
            }
            links_in.push(json!({ "page_id": p.page_id, "title": p.title }));
        }
        let mut m = Map::new();
        m.insert("links_out".into(), Value::Array(links_out));
        m.insert("links_in".into(), Value::Array(links_in));
        m.insert("withheld_links".into(), json!(withheld));
        m.insert("links_out_more".into(), json!(out_more));
        m.insert("links_in_more".into(), json!(in_more));
        Ok(m)
    }

    async fn read_page(
        &self,
        ctx: &ToolCallContext,
        args: &Map<String, Value>,
    ) -> Result<ToolOutput, ToolFailure> {
        let id = args
            .get("page_id")
            .and_then(Value::as_str)
            .and_then(parse_page_id)
            .ok_or_else(not_found)?;
        let mut conn = self.svc.conn().await.map_err(|_| db_failure())?;
        let own = store::page(&mut conn, id).await.map_err(|_| db_failure())?;
        let mut o = Map::new();
        let state;
        match own {
            Some(row)
                if row.workspace_id == ctx.workspace_id
                    && row.state != KnowledgePageState::Removed =>
            {
                drop(conn);
                if !self.visible(ctx, row.id, &row.sources, false).await {
                    return Err(not_found());
                }
                let checked = self.svc.check_page(row.id).await.unwrap_or(row.state);
                let mut conn = self.svc.conn().await.map_err(|_| db_failure())?;
                let row = store::page(&mut conn, id)
                    .await
                    .map_err(|_| db_failure())?
                    .ok_or_else(not_found)?;
                let marks = store::marks(&mut conn, id)
                    .await
                    .map_err(|_| db_failure())?;
                drop(conn);
                state = checked;
                o.insert("page_id".into(), json!(row.id));
                o.insert("title".into(), json!(row.title));
                o.insert(
                    "written_by".into(),
                    json!(store::author_text(row.written_by)),
                );
                o.insert("last_changed_at".into(), json!(row.last_changed_at));
                o.insert("state".into(), json!(store::state_text(checked)));
                let changes: Vec<Value> = marks
                    .iter()
                    .filter(|m| m.left_at.is_none())
                    .map(|m| {
                        let mut c = Map::new();
                        c.insert("location".into(), json!(m.location));
                        let read = row
                            .sources
                            .iter()
                            .find(|s| s.origin == m.origin && s.location == m.location)
                            .map(|s| s.version_read.clone())
                            .unwrap_or_default();
                        c.insert("version_read".into(), json!(read));
                        c.insert("newer_version".into(), json!(m.newer_version));
                        if let Some(b) = &m.changed_by {
                            c.insert("changed_by".into(), json!(b));
                        }
                        if let Some(a) = &m.changed_at {
                            c.insert("changed_at".into(), json!(a));
                        }
                        Value::Object(c)
                    })
                    .collect();
                o.insert("changes".into(), Value::Array(changes));
                o.insert(
                    "sources".into(),
                    json!(row
                        .sources
                        .iter()
                        .map(StoredSource::plain)
                        .collect::<Vec<_>>()),
                );
                o.insert("body_markdown".into(), json!(row.body));
                o.insert("tags".into(), json!(row.tags));
                o.insert("aliases".into(), json!(row.aliases));
                okf_fields(
                    &mut o,
                    &row.page_type,
                    &row.description,
                    row.okf_tier,
                    row.okf_status,
                    &row.stale_after,
                );
                let links = self.tool_links(ctx, &row).await.map_err(|_| db_failure())?;
                o.extend(links);
            }
            Some(_) => return Err(not_found()),
            None => {
                let read = crate::read::read_row(&mut conn, ctx.workspace_id, id)
                    .await
                    .map_err(|_| db_failure())?
                    .ok_or_else(not_found)?;
                if !self.visible(ctx, read.page_id, &read.sources, true).await {
                    return Err(not_found());
                }
                let view = self
                    .svc
                    .read_view(&mut conn, &read)
                    .await
                    .map_err(|_| db_failure())?;
                drop(conn);
                state = KnowledgePageState::Current;
                o.insert("page_id".into(), json!(read.page_id));
                o.insert("title".into(), json!(read.title));
                o.insert(
                    "written_by".into(),
                    json!(store::author_text(read.written_by)),
                );
                o.insert("last_changed_at".into(), json!(read.last_changed_at));
                o.insert("state".into(), json!("current"));
                o.insert("changes".into(), json!([]));
                o.insert(
                    "sources".into(),
                    json!(read
                        .sources
                        .iter()
                        .map(StoredSource::plain)
                        .collect::<Vec<_>>()),
                );
                o.insert("body_markdown".into(), json!(read.body));
                o.insert("tags".into(), json!(read.tags));
                o.insert("aliases".into(), json!(read.aliases));
                okf_fields(
                    &mut o,
                    &read.page_type,
                    &read.description,
                    read.okf_tier,
                    read.okf_status,
                    &read.stale_after,
                );
                self.folder_fields(&mut o, &read).await;
                o.insert("read_only".into(), json!(true));
                // Links of a read page stay in its folder (SA-KREAD-03); its pages are read pages.
                let mut links_out = Vec::new();
                for l in view.links_out.into_iter().take(MAX_TOOL_LINKS) {
                    links_out.push(match l.target {
                        Some(t) => json!({ "page_id": t.page_id, "title": t.title }),
                        None => json!({ "page_id": null, "name": l.name }),
                    });
                }
                o.insert("links_out".into(), Value::Array(links_out));
                o.insert(
                    "links_in".into(),
                    json!(view
                        .links_in
                        .iter()
                        .take(MAX_TOOL_LINKS)
                        .map(|p| json!({"page_id": p.page_id, "title": p.title}))
                        .collect::<Vec<_>>()),
                );
                o.insert("withheld_links".into(), json!(0));
                o.insert("links_out_more".into(), json!(0));
                o.insert("links_in_more".into(), json!(0));
            }
        }
        self.svc
            .journal(
                "page_read",
                Some(ctx.workspace_id),
                Some(ctx.run_id),
                "A session read a knowledge page.",
                json!({ "run_id": ctx.run_id, "page_id": id, "state": store::state_text(state) }),
            )
            .await;
        Ok(json_out(Value::Object(o)))
    }

    // ------------------------------------------------------------------ agent profile (H-7)

    fn index(&self) -> Option<Index> {
        let agent = self.svc.agent.as_ref()?;
        Some(Index::new(
            Arc::clone(agent),
            self.svc.journal.datastore().clone(),
        ))
    }

    async fn result_count(
        &self,
        src: &KnowledgeSourceConfig,
        ctx: &ToolCallContext,
    ) -> Result<u32, KnowledgeError> {
        let default = u32::from(src.default_results).clamp(1, MAX_RESULTS);
        let (Some(name), Some(scope)) = (&src.results_setting, ctx.scope_id) else {
            return Ok(default);
        };
        let agent = self
            .svc
            .agent
            .as_ref()
            .ok_or(KnowledgeError::ProfileMismatch)?;
        match agent.settings.number(scope, name).await {
            Ok(n) => Ok(u32::try_from(n.clamp(1, i64::from(MAX_RESULTS))).unwrap_or(default)),
            Err(SettingsReadError::Unavailable) => Err(KnowledgeError::DbUnavailable),
            Err(SettingsReadError::Unknown) => Ok(default),
        }
    }

    async fn agent_call(
        &self,
        ctx: &ToolCallContext,
        tool: &str,
        args: &Map<String, Value>,
    ) -> ToolOutput {
        let Some(index) = self.index() else {
            return text_out(AGENT_DOWN.to_owned());
        };
        let note_tool = tool.starts_with("note_");
        let unavailable = |code: &str| {
            tracing::error!(component = "knowledge", code, "knowledge_unavailable");
            text_out(if note_tool { NOTE_DOWN } else { AGENT_DOWN }.to_owned())
        };
        let result = if note_tool {
            self.note_call(ctx, &index, tool, args).await
        } else {
            self.source_call(ctx, &index, tool, args).await
        };
        match result {
            Ok(out) => out,
            Err(KnowledgeError::DbUnavailable) => unavailable("KNOW_DB_UNAVAILABLE"),
            Err(e) => text_out(format!("Refused: {}", e.code())),
        }
    }

    async fn source_call(
        &self,
        ctx: &ToolCallContext,
        index: &Index,
        tool: &str,
        args: &Map<String, Value>,
    ) -> Result<ToolOutput, KnowledgeError> {
        let agent = self
            .svc
            .agent
            .as_ref()
            .ok_or(KnowledgeError::ProfileMismatch)?;
        let (op, src) = agent
            .sources
            .iter()
            .find_map(|s| {
                ["search", "recent", "open"]
                    .into_iter()
                    .find(|op| tool_name(op, &s.name) == tool)
                    .map(|op| (op, s))
            })
            .ok_or(KnowledgeError::UnknownSource)?;
        match op {
            "search" => {
                let Some(query) = args
                    .get("query")
                    .and_then(Value::as_str)
                    .filter(|q| (1..=500).contains(&q.chars().count()))
                else {
                    return Ok(text_out(
                        "Refused: query must be a text of 1 to 500 characters.".to_owned(),
                    ));
                };
                if let Some(extra) = args.keys().find(|k| *k != "query") {
                    return Ok(text_out(format!("Refused: {extra} must be left out.")));
                }
                let count = self.result_count(src, ctx).await?;
                let question = agent.runs.question(ctx.run_id).await;
                let mut sources = vec![src.name.clone()];
                sources.extend(
                    agent
                        .notes_sources()
                        .map(|n| n.name.clone())
                        .filter(|n| *n != src.name),
                );
                let results = index
                    .search(&sources, ctx.scope_id, query, question.as_deref(), count)
                    .await?;
                Ok(text_out(render_results(&results)))
            }
            "recent" => {
                if let Some(extra) = args.keys().next() {
                    return Ok(text_out(format!("Refused: {extra} must be left out.")));
                }
                let count = self.result_count(src, ctx).await?;
                let items = index.recent(&src.name, ctx.scope_id, count).await?;
                if items.is_empty() {
                    return Ok(text_out("No items.".to_owned()));
                }
                let mut text = format!("{} items, newest first.", items.len());
                for (i, item) in items.iter().enumerate() {
                    text.push('\n');
                    text.push_str(&result_line(i + 1, item, ResultLabel::of(item.kind)));
                }
                Ok(text_out(text))
            }
            _ => {
                let Some(item_id) = args.get("item_id").and_then(Value::as_str) else {
                    return Ok(text_out("Refused: item_id must be a text.".to_owned()));
                };
                if let Some(extra) = args.keys().find(|k| *k != "item_id") {
                    return Ok(text_out(format!("Refused: {extra} must be left out.")));
                }
                let found = index.open(item_id, ctx.scope_id).await?;
                let vis = index.vis(src, ctx.scope_id).await?;
                let usable = found.filter(|i| {
                    i.source == src.name
                        && !i.retired
                        && (i.kind != ItemKind::Entries || i.verified == Some(true))
                        && (i.kind != ItemKind::ChatHistory
                            || ((vis.include_own || !i.own)
                                && !i.sender.as_ref().is_some_and(|s| vis.hidden.contains(s))))
                });
                let Some(item) = usable else {
                    return Ok(text_out(format!("No item {item_id} is available here.")));
                };
                agent.runs.item_opened(ctx.run_id, item.item_id).await;
                let label = ResultLabel::of(item.kind);
                Ok(text_out(format!(
                    "item {} | {} | {} | {}\n{}",
                    item.item_id,
                    label.text(),
                    when(&item),
                    who(&item, label),
                    cut_text(&item.text, OPEN_TEXT_CHARS)
                )))
            }
        }
    }

    async fn note_call(
        &self,
        ctx: &ToolCallContext,
        index: &Index,
        tool: &str,
        args: &Map<String, Value>,
    ) -> Result<ToolOutput, KnowledgeError> {
        let agent = self
            .svc
            .agent
            .as_ref()
            .ok_or(KnowledgeError::ProfileMismatch)?;
        let src = self.notes_source().ok_or(KnowledgeError::NoNotesSource)?;
        let text = args.get("text").and_then(Value::as_str);
        let note_id = args.get("note_id").and_then(Value::as_str);
        let refusal = |e: &KnowledgeError, id: Option<&str>| -> Option<String> {
            Some(match e {
                KnowledgeError::NoteLimit { max } => format!("Refused: this place already holds {max} notes, the limit of the configuration (max-notes-per-scope: {max}). Retire a note before you write another."),
                KnowledgeError::NoteTooLong { max, len } => format!("Refused: a note holds at most {max} characters (max-note-chars: {max}); this text holds {len}."),
                KnowledgeError::NoteEmpty => "Refused: a note needs a text.".to_owned(),
                KnowledgeError::NoteNotFound => format!("No note {} of yours exists here.", id.unwrap_or("")),
                _ => return None,
            })
        };
        let reason_word = |e: &KnowledgeError| match e {
            KnowledgeError::NoteLimit { .. } => Some("limit"),
            KnowledgeError::NoteTooLong { .. } => Some("too-long"),
            _ => None,
        };
        match tool {
            "note_write" => {
                let Some(text) = text else {
                    return Ok(text_out(
                        "Refused: text must be a text of at least 1 character.".to_owned(),
                    ));
                };
                match index
                    .note_write(&src.name, ctx.scope_id, text, ctx.run_id)
                    .await
                {
                    Ok(id) => {
                        agent.metrics.note_written(&src.name, "write");
                        Ok(text_out(format!("Note written: {id}")))
                    }
                    Err(e) => {
                        if let Some(r) = reason_word(&e) {
                            agent.metrics.note_refused(&src.name, r);
                        }
                        refusal(&e, None).map(text_out).ok_or(e)
                    }
                }
            }
            "note_replace" => {
                let (Some(id), Some(text)) = (note_id, text) else {
                    return Ok(text_out(
                        "Refused: note_id and text must be texts.".to_owned(),
                    ));
                };
                match index.note_replace(id, ctx.scope_id, text, ctx.run_id).await {
                    Ok(()) => {
                        agent.metrics.note_written(&src.name, "replace");
                        Ok(text_out(format!("Note replaced: {id}")))
                    }
                    Err(e) => {
                        if let Some(r) = reason_word(&e) {
                            agent.metrics.note_refused(&src.name, r);
                        }
                        refusal(&e, Some(id)).map(text_out).ok_or(e)
                    }
                }
            }
            _ => {
                let Some(id) = note_id else {
                    return Ok(text_out("Refused: note_id must be a text.".to_owned()));
                };
                match index.note_retire(id, ctx.scope_id).await {
                    Ok(()) => Ok(text_out(format!("Note retired: {id}"))),
                    Err(e) => refusal(&e, Some(id)).map(text_out).ok_or(e),
                }
            }
        }
    }
}

fn when(item: &KnowledgeItem) -> String {
    item.origin_time.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn who(item: &KnowledgeItem, label: ResultLabel) -> String {
    match label {
        ResultLabel::AgentNote => "your own note".to_owned(),
        ResultLabel::Entry => "a verified entry".to_owned(),
        ResultLabel::ChatHistory => {
            let name = item
                .sender_name
                .clone()
                .or_else(|| item.sender.clone())
                .unwrap_or_default();
            if item.from_program {
                format!("{name} (a program)")
            } else {
                name
            }
        }
    }
}

fn cut_text(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        format!("{} […]", cut(text, max))
    } else {
        text.to_owned()
    }
}

fn result_line(n: usize, item: &KnowledgeItem, label: ResultLabel) -> String {
    format!(
        "[{n}] item {} | {} | {} | {}\n{}",
        item.item_id,
        label.text(),
        when(item),
        who(item, label),
        cut_text(&item.text, RESULT_TEXT_CHARS)
    )
}

/// The search result text (wrightrun's, verbatim).
fn render_results(results: &[SearchResult]) -> String {
    if results.is_empty() {
        return "No results.".to_owned();
    }
    let mut text = format!("{} results.", results.len());
    for (i, r) in results.iter().enumerate() {
        text.push('\n');
        text.push_str(&result_line(i + 1, &r.item, r.label));
        if let Some(reply) = &r.reply {
            let label = ResultLabel::of(reply.kind);
            text.push_str(&format!(
                "\n    reply: item {} | {} | {}",
                reply.item_id,
                when(reply),
                who(reply, label)
            ));
            for line in cut_text(&reply.text, RESULT_TEXT_CHARS).lines() {
                text.push_str("\n    ");
                text.push_str(line);
            }
        }
    }
    text
}

#[async_trait::async_trait]
impl ToolProvider for KnowledgeTools {
    fn provider_id(&self) -> &'static str {
        "knowledge"
    }

    fn servers(&self) -> Vec<ServerSpec> {
        let tools = if self.svc.program.is_agent() {
            self.agent_specs()
        } else {
            Self::desktop_specs()
        };
        vec![ServerSpec {
            name: server(),
            tools,
        }]
    }

    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let agent = self.svc.program.is_agent();
        if agent && grant.caller != ToolCaller::HostedAgent {
            return Ok(Vec::new());
        }
        Ok(self
            .servers()
            .into_iter()
            .flat_map(|s| s.tools.into_iter().map(move |t| (s.name.clone(), t.name)))
            .map(|(server, tool)| ToolKey { server, tool })
            .collect())
    }

    fn check_arguments(&self, key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        let Some(args) = arguments.as_object() else {
            return Err(ArgumentProblem {
                at: String::new(),
                reason: "The arguments must be a JSON object.".to_owned(),
            });
        };
        if self.svc.program.is_agent() {
            return Ok(());
        }
        match key.tool.as_str() {
            SEARCH_PAGES if !check_search(args) => Err(ArgumentProblem {
                at: String::new(),
                reason: SEARCH_REASON.to_owned(),
            }),
            READ_PAGE => {
                let ok = args.len() == 1
                    && args
                        .get("page_id")
                        .and_then(Value::as_str)
                        .and_then(parse_page_id)
                        .is_some();
                if ok {
                    Ok(())
                } else {
                    Err(ArgumentProblem {
                        at: "/page_id".to_owned(),
                        reason: READ_REASON.to_owned(),
                    })
                }
            }
            _ => Ok(()),
        }
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        let args = arguments.as_object().cloned().unwrap_or_default();
        if self.svc.program.is_agent() {
            return Ok(self.agent_call(&ctx, &key.tool, &args).await);
        }
        match key.tool.as_str() {
            SEARCH_PAGES => self.search_pages(&ctx, &args).await,
            READ_PAGE => self.read_page(&ctx, &args).await,
            _ => Err(ToolFailure {
                code: "KNOW_PAGE_NOT_FOUND".to_owned(),
                message: NOT_AVAILABLE.to_owned(),
                retryable: false,
            }),
        }
    }

    async fn session_closed(&self, _run_id: RunId) {}
}
