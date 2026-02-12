//! The desktop rig: a `KnowledgeService` over a real SQLite journal and the in-memory fakes.

use std::sync::Arc;

use ordin_files::{FileVersionOrigin, PlaceChange, PlaceChangeKind};
use ordin_journal::Datastore;
use ordin_mcp_front::{CallVia, ProgressSink, ServerName, ToolCallContext, ToolKey};
use ordin_types::domain::places::{KnowledgeAuthor, KnowledgeSource};
use ordin_types::session::ToolCaller;
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, KnowledgePageId, PlaceId, RunId, ToolRequestId, WorkspaceId};

use super::fakes::{journal_entries, sqlite_journal, Fakes, TempDir};
use crate::model::{ProgramKind, WritePage};
use crate::service::KnowledgeService;
use crate::store::{self, PageRow};

pub(crate) struct Rig {
    pub svc: Arc<KnowledgeService>,
    pub f: Fakes,
    pub store: Datastore,
    pub w: WorkspaceId,
    pub place: PlaceId,
    _dir: TempDir,
}

pub(crate) fn src(location: &str, version: &str) -> KnowledgeSource {
    KnowledgeSource {
        origin: "place".to_owned(),
        location: location.to_owned(),
        version_read: version.to_owned(),
    }
}

pub(crate) fn src_kind(origin: &str, location: &str, version: &str) -> KnowledgeSource {
    KnowledgeSource {
        origin: origin.to_owned(),
        location: location.to_owned(),
        version_read: version.to_owned(),
    }
}

impl Rig {
    pub(crate) async fn desktop() -> Rig {
        let dir = TempDir::new();
        let (store, journal) = sqlite_journal(&dir).await;
        let f = Fakes::new();
        let place = PlaceId::new();
        f.files.add_place(place);
        f.rules.known_places.lock().unwrap().insert(place);
        let w = f.workspaces.add("Q3 supplier review", place);
        let svc = KnowledgeService::new(ProgramKind::Desktop, f.deps(journal, None))
            .await
            .expect("the service");
        Rig {
            svc,
            f,
            store,
            w,
            place,
            _dir: dir,
        }
    }

    /// A second workspace with its own place.
    pub(crate) fn workspace(&self, name: &str) -> (WorkspaceId, PlaceId) {
        let place = PlaceId::new();
        self.f.files.add_place(place);
        self.f.rules.known_places.lock().unwrap().insert(place);
        (self.f.workspaces.add(name, place), place)
    }

    /// A place source in this workspace's own folder.
    pub(crate) fn place_src(&self, path: &str, version: &str) -> KnowledgeSource {
        src(&format!("place:{}:{path}", self.place), version)
    }

    pub(crate) fn req(&self, title: &str, body: &str, sources: Vec<KnowledgeSource>) -> WritePage {
        WritePage {
            workspace_id: self.w,
            page_id: None,
            title: title.to_owned(),
            body_markdown: body.to_owned(),
            sources,
            run_id: Some(RunId::new()),
            tags: None,
            aliases: None,
            page_type: None,
            description: None,
        }
    }

    pub(crate) async fn write(&self, title: &str, body: &str) -> KnowledgePageId {
        self.svc
            .write_page(KnowledgeAuthor::Ordin, self.req(title, body, Vec::new()))
            .await
            .expect("write_page")
    }

    pub(crate) async fn write_with(
        &self,
        title: &str,
        body: &str,
        aliases: &[&str],
        tags: &[&str],
    ) -> KnowledgePageId {
        let mut req = self.req(title, body, Vec::new());
        req.aliases = Some(aliases.iter().map(|a| (*a).to_owned()).collect());
        req.tags = Some(tags.iter().map(|t| (*t).to_owned()).collect());
        self.svc
            .write_page(KnowledgeAuthor::Person, req)
            .await
            .expect("write_page")
    }

    pub(crate) async fn row(&self, id: KnowledgePageId) -> PageRow {
        let mut conn = self.svc.conn().await.unwrap();
        store::page(&mut conn, id)
            .await
            .unwrap()
            .expect("the page row")
    }

    pub(crate) async fn journal(&self) -> Vec<(String, serde_json::Value)> {
        journal_entries(&self.store).await
    }

    pub(crate) async fn kinds(&self) -> Vec<String> {
        self.journal().await.into_iter().map(|(k, _)| k).collect()
    }

    /// The `knowledge_pages_changed` events appended so far, as their `workspace_id` (KS-PC-1).
    pub(crate) async fn pages_changed(&self) -> Vec<String> {
        if self.svc.journal.last_seq() == 0 {
            return Vec::new();
        }
        self.svc
            .journal
            .events_since(0, 500)
            .await
            .expect("events")
            .into_iter()
            .filter(|e| e.kind == "knowledge_pages_changed")
            .map(|e| {
                e.data["workspace_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    pub(crate) fn text(&self, path: &str) -> String {
        self.f.files.text(self.place, path).unwrap_or_default()
    }

    /// The File Service reports a change in this workspace's own folder.
    pub(crate) async fn report(&self, path: &str, change: PlaceChangeKind, version: Option<u32>) {
        self.report_in(self.place, path, change, version).await;
    }

    pub(crate) async fn report_in(
        &self,
        place: PlaceId,
        path: &str,
        change: PlaceChangeKind,
        version: Option<u32>,
    ) {
        self.svc
            .on_place_change(PlaceChange {
                place_id: place,
                path: path.to_owned(),
                change,
                observed_at: Timestamp::now(),
                by_ordin: false,
                version,
                origin: version.map(|_| FileVersionOrigin::Person),
            })
            .await
            .expect("the change is applied");
    }

    /// Another app writes a file and the File Service reports it.
    pub(crate) async fn edit_outside(&self, path: &str, text: &str) {
        let created = self.f.files.text(self.place, path).is_none();
        let v = self.f.files.external_write(self.place, path, text);
        let kind = if created {
            PlaceChangeKind::Created
        } else {
            PlaceChangeKind::Modified
        };
        self.report(path, kind, Some(v.version)).await;
    }

    pub(crate) fn ctx(&self, caller: ToolCaller, agent: AgentInstanceId) -> ToolCallContext {
        ToolCallContext {
            run_id: RunId::new(),
            workspace_id: self.w,
            agent_instance_id: agent,
            caller,
            scope_id: None,
            coding_task_id: None,
            call_id: ToolRequestId::new(),
            correlation_id: "test".to_owned(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
            cancel: tokio_util::sync::CancellationToken::new(),
            progress: ProgressSink::detached(),
            only_needed_parts: false,
            via: CallVia::Harness,
        }
    }

    pub(crate) fn key(tool: &str) -> ToolKey {
        ToolKey {
            server: ServerName::new("knowledge").unwrap(),
            tool: tool.to_owned(),
        }
    }

    pub(crate) async fn call(
        &self,
        ctx: ToolCallContext,
        tool: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ordin_mcp_front::ToolFailure> {
        let provider = self.svc.tool_provider();
        provider
            .call(ctx, &Self::key(tool), args)
            .await
            .map(|o| o.structured.unwrap_or(serde_json::Value::Null))
    }

    pub(crate) async fn index(&self) {
        self.svc.index_pass().await.expect("an index pass");
    }

    pub(crate) async fn exec(&self, sql: &str) {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(self.svc.pool().unwrap())
            .await
            .expect("statement");
    }
}
