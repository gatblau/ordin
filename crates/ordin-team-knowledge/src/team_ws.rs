//! A team workspace (TeamKnowledgeService.md amendment January 2026, ADR-121): TW-A to TW-J.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use git2::Oid;
use ordin_types::domain::places::{CopyDestination, CopyFormat, CopyOriginal};
use ordin_types::domain::{
    FolderAccess, Grantee, SourceLink, SourceLinkKind, SourceLinkState, TeamAccess,
    WorkingCopyState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::ContentRef;
use ordin_types::{
    AgentInstanceId, Cursor, Page, PlaceId, RunId, SourceLinkId, TeamActivityId, TeamGrantId,
    WorkspaceId,
};
use ordin_workspaces::{
    GrantDecision, LinkLocation, NewPendingGrant, PendingAccess, Reach, WorkspaceError,
};
use sqlx::Row;

use crate::error::TeamError;
use crate::files::{
    self, PathKind, TeamAccessEntry, TeamActivityFile, TeamAgentEntry, TeamPlaceEntry,
    TeamResultsEntry, TeamWorkspaceFile,
};
use crate::git;
use crate::model::*;
use crate::service::{parse_origin, sha256_hex, split_url, TeamKnowledgeService, Tip};
use crate::store::{self, Binding, ShareScope};
use crate::sync::Plan;
use crate::text::WorkspaceLines;

/// At most 5,000 activity files and 5,000 results taken in per get (Configuration additions).
const MAX_PER_GET: usize = 5_000;
/// TW-J step 2.
const HISTORY_LIMIT: usize = 2_000;
const ACTIVITY_PAGE: usize = 50;

/// One row of `team_workspaces`.
#[derive(Debug, Clone)]
pub(crate) struct TeamRow {
    pub state: String,
    pub workspace_key: String,
    pub turned_on_here: bool,
    pub synced: Option<TeamWorkspaceFile>,
    pub results_kind: String,
    pub results_place_key: Option<String>,
}

/// A setting of the description (TW-D step 2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Setting {
    Name,
    Place(String),
    Agent(String),
    Results,
}

fn uuid7() -> String {
    uuid::Uuid::now_v7().hyphenated().to_string()
}

fn access_text(a: FolderAccess) -> &'static str {
    match a {
        FolderAccess::Read => "read",
        FolderAccess::ReadAndChange => "read_and_change",
    }
}

fn access_of(raw: &str) -> FolderAccess {
    if raw == "read_and_change" {
        FolderAccess::ReadAndChange
    } else {
        FolderAccess::Read
    }
}

/// The places a team workspace shares (SA-TEAM-TW-06).
fn shared_link(l: &SourceLink, own_link: Option<SourceLinkId>) -> bool {
    l.state == SourceLinkState::Active
        && Some(l.id) != own_link
        && files::place_kind_name(l.kind).is_some()
}

/// A link a results place may be (SA-TEAM-TW-05): a SharePoint item or a Confluence page.
fn results_link_ok(l: &SourceLink) -> bool {
    l.state == SourceLinkState::Active
        && match l.kind {
            SourceLinkKind::SharepointLocation => {
                files::location_ok("sharepoint", &l.origin_location)
            }
            SourceLinkKind::ConfluenceLocation => l.origin_location.starts_with("confluence:page/"),
            _ => false,
        }
}

/// The results choices of TW-A step 5 and TW-K step 1: the team folder first, named `results_name`,
/// then each link TW-A step 4 accepts, in the links' order.
fn results_options(results_name: String, links: &[SourceLink]) -> Vec<TeamResultsOption> {
    let mut choices = vec![TeamResultsOption {
        choice: TeamResultsChoice::TeamFolder {},
        name: results_name,
    }];
    for l in links.iter().filter(|l| results_link_ok(l)) {
        choices.push(TeamResultsOption {
            choice: TeamResultsChoice::Link {
                source_link_id: l.id,
            },
            name: l.display_name.clone(),
        });
    }
    choices
}

/// TW-G step 3: the copy destination a results link's location names, when it names a SharePoint
/// folder or a Confluence page.
fn results_destination(location: LinkLocation) -> Option<CopyDestination> {
    match location {
        LinkLocation::Sharepoint { location } if location.is_folder => {
            location
                .item_id
                .map(|folder| CopyDestination::SharepointFile {
                    site: location.site_id,
                    library: location.drive_id,
                    folder,
                })
        }
        LinkLocation::Confluence { location } => {
            location
                .page_id
                .map(|parent_page_id| CopyDestination::ConfluenceAttachment {
                    space: location.space_key,
                    parent_page_id,
                })
        }
        _ => None,
    }
}

fn ws_error(e: &WorkspaceError, code: &'static str) -> TeamError {
    TeamError::per_cause(code, e.retryable())
}

impl TeamKnowledgeService {
    // ---- rows ----

    pub(crate) async fn team_row(&self, ws: WorkspaceId) -> Result<Option<TeamRow>, TeamError> {
        let row = sqlx::query("SELECT * FROM team_workspaces WHERE workspace_id = ?1")
            .bind(ws.to_string())
            .fetch_optional(self.pool()?)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let synced: Option<String> = row.try_get("synced_description")?;
        Ok(Some(TeamRow {
            state: row.try_get("state")?,
            workspace_key: row.try_get("workspace_key")?,
            turned_on_here: row.try_get::<i64, _>("turned_on_here")? == 1,
            synced: synced.and_then(|s| serde_json::from_str(&s).ok()),
            results_kind: row.try_get("results_kind")?,
            results_place_key: row.try_get("results_place_key")?,
        }))
    }

    async fn team_ws(&self, ws: WorkspaceId) -> Result<TeamRow, TeamError> {
        match self.team_row(ws).await? {
            Some(row) if row.state == "team" => Ok(row),
            _ => Err(TeamError::permanent("TEAM_WS_NOT_TEAM")),
        }
    }

    async fn set_synced(&self, ws: WorkspaceId, file: &TeamWorkspaceFile) -> Result<(), TeamError> {
        sqlx::query("UPDATE team_workspaces SET synced_description = ?2 WHERE workspace_id = ?1")
            .bind(ws.to_string())
            .bind(store::json(file))
            .execute(self.pool()?)
            .await?;
        Ok(())
    }

    async fn set_results(
        &self,
        ws: WorkspaceId,
        kind: &str,
        key: Option<&str>,
    ) -> Result<(), TeamError> {
        sqlx::query("UPDATE team_workspaces SET results_kind = ?2, results_place_key = ?3 WHERE workspace_id = ?1")
            .bind(ws.to_string())
            .bind(kind)
            .bind(key)
            .execute(self.pool()?)
            .await?;
        Ok(())
    }

    /// (key, link, state) of `team_workspace_places`.
    async fn place_rows(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<(String, Option<SourceLinkId>, String)>, TeamError> {
        let rows = sqlx::query("SELECT key, source_link_id, state FROM team_workspace_places WHERE workspace_id = ?1 ORDER BY key")
            .bind(ws.to_string())
            .fetch_all(self.pool()?)
            .await?;
        rows.iter()
            .map(|r| {
                let link: Option<String> = r.try_get("source_link_id")?;
                Ok((
                    r.try_get("key")?,
                    link.and_then(|l| SourceLinkId::parse(&l).ok()),
                    r.try_get("state")?,
                ))
            })
            .collect()
    }

    async fn put_place_row(
        &self,
        ws: WorkspaceId,
        key: &str,
        link: Option<SourceLinkId>,
        state: &str,
    ) -> Result<(), TeamError> {
        sqlx::query(
            "INSERT INTO team_workspace_places (workspace_id, key, source_link_id, state) VALUES (?1, ?2, ?3, ?4) \
             ON CONFLICT (workspace_id, key) DO UPDATE SET source_link_id = excluded.source_link_id, state = excluded.state",
        )
        .bind(ws.to_string())
        .bind(key)
        .bind(link.map(|l| l.to_string()))
        .bind(state)
        .execute(self.pool()?)
        .await?;
        Ok(())
    }

    /// (key, agent, state, not_found_code, declined_changed_at) of `team_workspace_agents`.
    async fn agent_rows(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<(String, Option<AgentInstanceId>, String, Option<Timestamp>)>, TeamError> {
        let rows = sqlx::query("SELECT key, agent_instance_id, state, declined_changed_at FROM team_workspace_agents WHERE workspace_id = ?1 ORDER BY key")
            .bind(ws.to_string())
            .fetch_all(self.pool()?)
            .await?;
        rows.iter()
            .map(|r| {
                let agent: Option<String> = r.try_get("agent_instance_id")?;
                Ok((
                    r.try_get("key")?,
                    agent.and_then(|a| AgentInstanceId::parse(&a).ok()),
                    r.try_get("state")?,
                    store::ts(r.try_get("declined_changed_at")?),
                ))
            })
            .collect()
    }

    async fn put_agent_row(
        &self,
        ws: WorkspaceId,
        key: &str,
        agent: Option<AgentInstanceId>,
        state: &str,
        not_found_code: Option<&str>,
    ) -> Result<(), TeamError> {
        sqlx::query(
            "INSERT INTO team_workspace_agents (workspace_id, key, agent_instance_id, state, not_found_code) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (workspace_id, key) DO UPDATE SET agent_instance_id = COALESCE(excluded.agent_instance_id, team_workspace_agents.agent_instance_id), \
             state = excluded.state, not_found_code = excluded.not_found_code",
        )
        .bind(ws.to_string())
        .bind(key)
        .bind(agent.map(|a| a.to_string()))
        .bind(state)
        .bind(not_found_code)
        .execute(self.pool()?)
        .await?;
        Ok(())
    }

    /// The team folder files' records (`team_workspace_files`): path → (kind, blob, digest).
    async fn file_records(
        &self,
        ws: WorkspaceId,
    ) -> Result<HashMap<String, (String, String, Option<String>)>, TeamError> {
        let rows = sqlx::query("SELECT path, kind, blob, local_sha256 FROM team_workspace_files WHERE workspace_id = ?1")
            .bind(ws.to_string())
            .fetch_all(self.pool()?)
            .await?;
        let mut out = HashMap::new();
        for r in rows {
            out.insert(
                r.try_get("path")?,
                (
                    r.try_get("kind")?,
                    r.try_get("blob")?,
                    r.try_get("local_sha256")?,
                ),
            );
        }
        Ok(out)
    }

    async fn put_file_record(
        &self,
        ws: WorkspaceId,
        path: &str,
        kind: &str,
        blob: &str,
        sha: Option<&str>,
    ) -> Result<(), TeamError> {
        sqlx::query(
            "INSERT INTO team_workspace_files (workspace_id, path, kind, blob, local_sha256, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT (workspace_id, path) DO UPDATE SET kind = excluded.kind, blob = excluded.blob, \
             local_sha256 = excluded.local_sha256, synced_at = excluded.synced_at",
        )
        .bind(ws.to_string())
        .bind(path)
        .bind(kind)
        .bind(blob)
        .bind(sha)
        .bind(store::now())
        .execute(self.pool()?)
        .await?;
        Ok(())
    }

    async fn drop_file_record(&self, ws: WorkspaceId, path: &str) -> Result<(), TeamError> {
        sqlx::query("DELETE FROM team_workspace_files WHERE workspace_id = ?1 AND path = ?2")
            .bind(ws.to_string())
            .bind(path)
            .execute(self.pool()?)
            .await?;
        Ok(())
    }

    /// TW-H step 2's rows (also on a removed own link).
    pub(crate) async fn delete_team_rows(&self, ws: WorkspaceId) -> Result<(), TeamError> {
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        for table in [
            "team_activity",
            "team_workspace_files",
            "team_workspace_places",
            "team_workspace_agents",
            "team_workspaces",
        ] {
            // The table names are the fixed list above; the workspace is bound.
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE workspace_id = ?1"
            )))
            .bind(ws.to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    // ---- who (SA-TEAM-TW-02) ----

    // spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-TW-02
    // The person's GitHub login (`PersonProfile.github_login`) when present, for every host; else the
    // credential's username unless it is the transport's fixed name ("x-access-token" on GitHub,
    // "oauth2" on GitLab, ProjectService.md); else `TEAM_SIGN_IN_NEEDED`.
    /// The login of this person on the folder's host; `TEAM_SIGN_IN_NEEDED` when none can be read.
    pub(crate) async fn login(&self, host: &str) -> Result<String, TeamError> {
        #[cfg(test)]
        if let Some(login) = crate::service::lock(&self.seams).login.clone() {
            return Ok(login);
        }
        if let Ok(person) = self.deps.repos.person().await {
            if let Some(login) = person.github_login.filter(|l| files::login_ok(l)) {
                return Ok(login);
            }
        }
        match self.deps.repos.git_credentials(host).await {
            Ok(c)
                if files::login_ok(&c.username)
                    && c.username != "x-access-token"
                    && c.username != "oauth2" =>
            {
                Ok(c.username)
            }
            _ => Err(TeamError::of("TEAM_SIGN_IN_NEEDED")),
        }
    }

    // ---- the description now (TW-B step 1) ----

    /// Renders the description from the workspace now. Keys of new places and agents are minted;
    /// with `persist` they are stored. Stamps follow TW-B step 2 against `synced`.
    async fn render_description(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        row: &TeamRow,
        login: &str,
        persist: bool,
    ) -> Result<TeamWorkspaceFile, TeamError> {
        let workspace = self.active_workspace(ws).await?;
        let links = self
            .deps
            .workspaces
            .source_links(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WORKSPACE_NOT_FOUND"))?;
        let place_rows = self.place_rows(ws).await?;
        let agent_rows = self.agent_rows(ws).await?;
        let results_folder = self.deps.workspaces.results_folder(ws).await.ok().flatten();
        let now = Timestamp::now();
        let synced = row.synced.as_ref();

        let mut places = Vec::new();
        let mut place_by_id: HashMap<PlaceId, String> = HashMap::new();
        for link in links
            .iter()
            .filter(|l| shared_link(l, Some(b.source_link_id)))
        {
            let key = match place_rows.iter().find(|(_, l, _)| *l == Some(link.id)) {
                Some((k, _, _)) => k.clone(),
                None => {
                    let k = uuid7();
                    if persist {
                        self.put_place_row(ws, &k, Some(link.id), "linked").await?;
                    }
                    k
                }
            };
            if let Some(place) = link.place_id {
                place_by_id.insert(place, key.clone());
            }
            let kind = files::place_kind_name(link.kind)
                .unwrap_or("sharepoint")
                .to_owned();
            let mut entry = TeamPlaceEntry {
                key: key.clone(),
                kind,
                location: link.origin_location.clone(),
                display_name: link.display_name.chars().take(120).collect(),
                changed_by: login.to_owned(),
                changed_at: now,
            };
            if let Some(s) = synced.and_then(|s| s.places.iter().find(|p| p.key == key)) {
                if (
                    s.kind.as_str(),
                    s.location.as_str(),
                    s.display_name.as_str(),
                ) == (
                    entry.kind.as_str(),
                    entry.location.as_str(),
                    entry.display_name.as_str(),
                ) {
                    entry.changed_by = s.changed_by.clone();
                    entry.changed_at = s.changed_at;
                }
            }
            places.push(entry);
        }
        // Places a colleague removed whose copy holds unsent changes stay in the description.
        places.sort_by(|a, b| a.key.cmp(&b.key));

        let grants = self
            .deps
            .workspaces
            .grants(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WORKSPACE_NOT_FOUND"))?;
        let here = self
            .deps
            .agents
            .agents_in_workspace(ws)
            .await
            .map_err(|e| TeamError::per_cause("TEAM_WS_AGENT_NOT_ADDED", e.retryable))?;
        let mut agents = Vec::new();
        for agent in here {
            let Some((repository, id, version)) = agent.identity.clone() else {
                continue;
            };
            let key = match agent_rows
                .iter()
                .find(|(_, a, _, _)| *a == Some(agent.agent_instance_id))
            {
                Some((k, _, _, _)) => k.clone(),
                None => {
                    let k = uuid7();
                    if persist {
                        self.put_agent_row(ws, &k, Some(agent.agent_instance_id), "present", None)
                            .await?;
                    }
                    k
                }
            };
            let mut access: Vec<TeamAccessEntry> = grants
                .iter()
                .filter(|g| {
                    g.grantee
                        == Grantee::AgentInstance {
                            agent_instance_id: agent.agent_instance_id,
                        }
                })
                .filter_map(|g| {
                    let place = if g.place_id == workspace.own_folder_place_id {
                        "own".to_owned()
                    } else if Some(g.place_id) == results_folder {
                        "results".to_owned()
                    } else {
                        place_by_id.get(&g.place_id)?.clone()
                    };
                    Some(TeamAccessEntry {
                        place,
                        access: access_text(g.access).to_owned(),
                    })
                })
                .collect();
            access.sort_by(|a, b| a.place.cmp(&b.place));
            access.dedup_by(|a, b| a.place == b.place);
            let mut entry = TeamAgentEntry {
                key: key.clone(),
                repository,
                id,
                version,
                name: agent.display_name.chars().take(120).collect(),
                access,
                changed_by: login.to_owned(),
                changed_at: now,
            };
            if let Some(s) = synced.and_then(|s| s.agents.iter().find(|a| a.key == key)) {
                if agent_value(s) == agent_value(&entry) {
                    entry.changed_by = s.changed_by.clone();
                    entry.changed_at = s.changed_at;
                }
            }
            agents.push(entry);
        }
        agents.sort_by(|a, b| a.key.cmp(&b.key));

        let mut results = TeamResultsEntry {
            kind: row.results_kind.clone(),
            place: row.results_place_key.clone(),
            changed_by: login.to_owned(),
            changed_at: now,
        };
        if let Some(s) = synced {
            if (s.results.kind.as_str(), &s.results.place)
                == (results.kind.as_str(), &results.place)
            {
                results.changed_by = s.results.changed_by.clone();
                results.changed_at = s.results.changed_at;
            }
        }
        let name: String = workspace.name.chars().take(80).collect();
        let (name_changed_by, name_changed_at) = match synced {
            Some(s) if s.name == name => (s.name_changed_by.clone(), s.name_changed_at),
            _ => (login.to_owned(), now),
        };
        Ok(TeamWorkspaceFile {
            format: files::DESCRIPTION_FORMAT,
            workspace_key: row.workspace_key.clone(),
            name,
            name_changed_by,
            name_changed_at,
            places,
            agents,
            results,
        })
    }

    // ---- TW-A ----

    /// TW-A steps 1–4 shared by the preview and the share. Returns the binding (existing or as it
    /// would be) and the tip of the folder.
    async fn share_checks(
        &self,
        ws: WorkspaceId,
        choice: &TeamFolderChoice,
    ) -> Result<(Binding, Tip, Option<std::path::PathBuf>), TeamError> {
        let workspace = self.active_workspace(ws).await?;
        if workspace.state != ordin_types::domain::WorkspaceState::Active {
            return Err(TeamError::permanent("TEAM_WORKSPACE_NOT_FOUND"));
        }
        if let Some(row) = self.team_row(ws).await? {
            if row.state == "team" {
                return Err(TeamError::permanent("TEAM_WS_ALREADY_TEAM"));
            }
            if store::open_share(self.pool()?, ws).await?.is_some() {
                return Err(TeamError::permanent("TEAM_SHARE_PENDING"));
            }
        }
        let (binding, tip, temp) = match store::binding(self.pool()?, ws).await? {
            Some(b) => {
                let same = match choice {
                    TeamFolderChoice::Link { source_link_id } => {
                        *source_link_id == b.source_link_id
                    }
                    TeamFolderChoice::NewFolder { url, branch, path } => split_url(url)
                        .is_some_and(|(h, r)| {
                            h == b.host.to_ascii_lowercase()
                                && r.eq_ignore_ascii_case(&b.repository)
                                && *branch == b.branch
                                && path.trim_end_matches('/') == b.folder
                        }),
                };
                if !same {
                    return Err(TeamError::permanent("TEAM_ALREADY_ON"));
                }
                let tip = self.fetch_folder(&b).await?;
                (b, tip, None)
            }
            None => {
                let (link, host, repository, branch, folder) =
                    self.resolve_choice(ws, choice, false).await?;
                let access = self.access_of(&host, &repository).await?;
                let temp = self.deps.clones_dir.join(format!("open-{}.git", uuid7()));
                let fetched = self.fetch(temp.clone(), &host, &repository, &branch).await;
                let tip = match fetched {
                    Ok(t) => self.tip_files(temp.clone(), t, &folder).await,
                    Err(e) => Err(e),
                };
                let tip = match tip {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(&temp);
                        return Err(e);
                    }
                };
                let b = Binding {
                    workspace_id: ws,
                    source_link_id: link.unwrap_or_default(),
                    host,
                    repository,
                    branch,
                    folder,
                    access,
                    enabled_at: Timestamp::now(),
                    last_checked_at: None,
                    last_checked_commit: None,
                    access_checked_at: None,
                    available: None,
                    to_share: None,
                    held_for_check: 0,
                    folder_large: false,
                    last_error_code: None,
                    last_get: None,
                    last_share: None,
                };
                (b, tip, Some(temp))
            }
        };
        let refuse = |code| {
            if let Some(t) = &temp {
                let _ = std::fs::remove_dir_all(t);
            }
            Err(TeamError::permanent(code))
        };
        if binding.access == TeamAccess::ReadOnly {
            return refuse("TEAM_READ_ONLY");
        }
        if tip.files.contains_key(files::DESCRIPTION_FILE) {
            return refuse("TEAM_WS_FOLDER_TAKEN");
        }
        Ok((binding, tip, temp))
    }

    /// TW-A step 4.
    async fn check_results_choice(
        &self,
        ws: WorkspaceId,
        results: &TeamResultsChoice,
    ) -> Result<Option<SourceLink>, TeamError> {
        match results {
            TeamResultsChoice::TeamFolder {} => Ok(None),
            TeamResultsChoice::Link { source_link_id } => {
                let links = self
                    .deps
                    .workspaces
                    .source_links(ws)
                    .await
                    .map_err(|_| TeamError::permanent("TEAM_WS_RESULTS_NOT_SUITABLE"))?;
                links
                    .into_iter()
                    .find(|l| l.id == *source_link_id && results_link_ok(l))
                    .map(Some)
                    .ok_or_else(|| TeamError::permanent("TEAM_WS_RESULTS_NOT_SUITABLE"))
            }
        }
    }

    /// The share preview's lines from the workspace now (TW-A step 5).
    async fn preview_parts(
        &self,
        ws: WorkspaceId,
        b: &Binding,
    ) -> Result<
        (
            Vec<TeamPlaceLine>,
            Vec<String>,
            Vec<TeamAgentLine>,
            Vec<TeamResultsOption>,
        ),
        TeamError,
    > {
        let links = self
            .deps
            .workspaces
            .source_links(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WORKSPACE_NOT_FOUND"))?;
        let own_link = (b.source_link_id != SourceLinkId::default()).then_some(b.source_link_id);
        let mut places = Vec::new();
        let mut stays = Vec::new();
        let mut names: HashMap<PlaceId, String> = HashMap::new();
        for l in &links {
            if shared_link(l, own_link) {
                places.push(TeamPlaceLine {
                    key: uuid7(),
                    display_name: l.display_name.clone(),
                    kind: l.kind,
                    reach: None,
                });
                if let Some(p) = l.place_id {
                    names.insert(p, l.display_name.clone());
                }
            } else if l.state == SourceLinkState::Active
                && matches!(
                    l.kind,
                    SourceLinkKind::LocalFolder | SourceLinkKind::GitRepositoryFolder
                )
            {
                stays.push(l.display_name.clone());
            }
        }
        let workspace = self.active_workspace(ws).await?;
        let grants = self
            .deps
            .workspaces
            .grants(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WORKSPACE_NOT_FOUND"))?;
        let here = self
            .deps
            .agents
            .agents_in_workspace(ws)
            .await
            .map_err(|e| TeamError::per_cause("TEAM_WS_AGENT_NOT_ADDED", e.retryable))?;
        let results_name = format!("{} › results", b.header_line());
        let mut agents = Vec::new();
        for a in here {
            if a.identity.is_none() {
                stays.push(a.display_name.clone());
                continue;
            }
            let mut access: Vec<TeamAccessLine> = grants
                .iter()
                .filter(|g| {
                    g.grantee
                        == Grantee::AgentInstance {
                            agent_instance_id: a.agent_instance_id,
                        }
                })
                .filter_map(|g| {
                    let name = if g.place_id == workspace.own_folder_place_id {
                        workspace.name.clone()
                    } else {
                        names.get(&g.place_id)?.clone()
                    };
                    Some(TeamAccessLine {
                        place_name: name,
                        access: g.access,
                    })
                })
                .collect();
            // TW-A step 5: the results grant step 6 makes before it renders.
            access.push(TeamAccessLine {
                place_name: results_name.clone(),
                access: FolderAccess::ReadAndChange,
            });
            agents.push(TeamAgentLine {
                key: uuid7(),
                name: a.display_name,
                access,
                found: None,
            });
        }
        let choices = results_options(results_name, &links);
        Ok((places, stays, agents, choices))
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-A
    pub async fn workspace_share_preview(
        &self,
        ws: WorkspaceId,
        choice: TeamFolderChoice,
    ) -> Result<TeamWorkspaceSharePreview, TeamError> {
        let (b, tip, temp) = self.share_checks(ws, &choice).await?;
        let knowledge = async {
            let name = self.person_name().await;
            let local = self.export_local(ws, &name).await?;
            let records = if temp.is_none() {
                store::records(self.pool()?, ws).await?
            } else {
                HashMap::new()
            };
            Ok::<_, TeamError>(self.item_plan(&local, &tip, &records).counts)
        }
        .await;
        let parts = self.preview_parts(ws, &b).await;
        if let Some(t) = &temp {
            let _ = std::fs::remove_dir_all(t);
        }
        let (places, stays_here, agents, results_choices) = parts?;
        // The login is needed for the share; the preview says so before Share is pressed.
        self.login(&b.host).await?;
        Ok(TeamWorkspaceSharePreview {
            folder_name: b.header_line(),
            places,
            stays_here,
            agents,
            knowledge: knowledge?,
            results_choices,
        })
    }

    pub async fn share_workspace(
        &self,
        ws: WorkspaceId,
        choice: TeamFolderChoice,
        results: TeamResultsChoice,
    ) -> Result<TeamKnowledgeStatus, TeamError> {
        let (b, _, temp) = self.share_checks(ws, &choice).await?;
        if let Some(t) = &temp {
            let _ = std::fs::remove_dir_all(t);
        }
        let results_link = self.check_results_choice(ws, &results).await?;
        self.login(&b.host).await?;
        self.start_long(ws, TeamBusy::Sharing, move |me| async move {
            me.share_workspace_locked(ws, choice, results_link).await
        })
        .await
    }

    /// TW-A step 6 and the first share.
    async fn share_workspace_locked(
        &self,
        ws: WorkspaceId,
        choice: TeamFolderChoice,
        results_link: Option<SourceLink>,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let results_folder = self
            .deps
            .workspaces
            .make_results_folder(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WS_ALLOW_FAILED"))?;
        let here = self
            .deps
            .agents
            .agents_in_workspace(ws)
            .await
            .map_err(|e| TeamError::per_cause("TEAM_WS_ALLOW_FAILED", e.retryable))?;
        let mut made: Vec<AgentInstanceId> = Vec::new();
        for a in here.iter().filter(|a| a.identity.is_some()) {
            let grantee = Grantee::AgentInstance {
                agent_instance_id: a.agent_instance_id,
            };
            if let Err(e) = self
                .deps
                .workspaces
                .set_grant(
                    ws,
                    grantee,
                    results_folder.id,
                    Some(FolderAccess::ReadAndChange),
                )
                .await
            {
                for agent in &made {
                    let _ = self
                        .deps
                        .workspaces
                        .set_grant(
                            ws,
                            Grantee::AgentInstance {
                                agent_instance_id: *agent,
                            },
                            results_folder.id,
                            None,
                        )
                        .await;
                }
                return Err(ws_error(&e, "TEAM_WS_ALLOW_FAILED"));
            }
            made.push(a.agent_instance_id);
        }
        let mut turned_on_here = false;
        if store::binding(&pool, ws).await?.is_none() {
            if let Err(e) = self.turn_on_locked(ws, &choice).await {
                for agent in &made {
                    let _ = self
                        .deps
                        .workspaces
                        .set_grant(
                            ws,
                            Grantee::AgentInstance {
                                agent_instance_id: *agent,
                            },
                            results_folder.id,
                            None,
                        )
                        .await;
                }
                return Err(e);
            }
            turned_on_here = true;
        }
        let (kind, key) = match &results_link {
            None => ("team_folder", None),
            Some(l) => {
                let key = uuid7();
                (
                    if l.kind == SourceLinkKind::SharepointLocation {
                        "sharepoint"
                    } else {
                        "confluence"
                    },
                    Some((key, l.id)),
                )
            }
        };
        let existing = self.team_row(ws).await?;
        if existing.is_none() {
            sqlx::query(
                "INSERT INTO team_workspaces (workspace_id, state, workspace_key, turned_on_here, synced_description, results_kind, results_place_key, created_at) \
                 VALUES (?1, 'proposed', ?2, ?3, NULL, ?4, ?5, ?6)",
            )
            .bind(ws.to_string())
            .bind(ws.to_string())
            .bind(i64::from(turned_on_here))
            .bind(kind)
            .bind(key.as_ref().map(|(k, _)| k.clone()))
            .bind(store::now())
            .execute(&pool)
            .await?;
        } else {
            self.set_results(ws, kind, key.as_ref().map(|(k, _)| k.as_str()))
                .await?;
        }
        if let Some((k, link)) = &key {
            self.put_place_row(ws, k, Some(*link), "linked").await?;
        }
        self.share_now(ws, ShareScope::Workspace).await.map(|_| ())
    }

    /// The card's lines for TW-A step 6.
    pub(crate) async fn share_preview_lines(
        &self,
        ws: WorkspaceId,
        b: &Binding,
    ) -> Result<WorkspaceLines, TeamError> {
        let (places, stays, agents, _) = self.preview_parts(ws, b).await?;
        let row = self.team_row(ws).await?;
        let results = match row.as_ref().map(|r| r.results_kind.as_str()) {
            Some("sharepoint" | "confluence") => {
                let key = row.as_ref().and_then(|r| r.results_place_key.clone());
                let links = self
                    .deps
                    .workspaces
                    .source_links(ws)
                    .await
                    .unwrap_or_default();
                let rows = self.place_rows(ws).await?;
                rows.iter()
                    .find(|(k, _, _)| Some(k) == key.as_ref())
                    .and_then(|(_, l, _)| links.iter().find(|x| Some(x.id) == *l))
                    .map(|l| l.display_name.clone())
                    .unwrap_or_default()
            }
            _ => format!("{} › results", b.header_line()),
        };
        Ok(WorkspaceLines {
            places: places.into_iter().map(|p| p.display_name).collect(),
            stays,
            agents: agents
                .into_iter()
                .map(|a| {
                    let change: Vec<String> = a
                        .access
                        .iter()
                        .filter(|x| {
                            x.access == FolderAccess::ReadAndChange
                                && !x.place_name.ends_with("› results")
                        })
                        .map(|x| x.place_name.clone())
                        .collect();
                    let read: Vec<String> = a
                        .access
                        .iter()
                        .filter(|x| x.access == FolderAccess::Read)
                        .map(|x| x.place_name.clone())
                        .collect();
                    let mut parts = Vec::new();
                    if !change.is_empty() {
                        parts.push(format!("can read and change {}", change.join(", ")));
                    }
                    if !read.is_empty() {
                        parts.push(format!("can read {}", read.join(", ")));
                    }
                    if parts.is_empty() {
                        a.name
                    } else {
                        format!("{} ({})", a.name, parts.join("; "))
                    }
                })
                .collect(),
            results,
        })
    }

    // ---- TW-B, TW-F step 3, TW-G step 2: what a share carries ----

    pub(crate) async fn plan_team_parts(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        tip: &Tip,
        scope: ShareScope,
        plan: &mut Plan,
    ) -> Result<(), TeamError> {
        let Some(row) = self.team_row(ws).await? else {
            return Ok(());
        };
        let records = self.file_records(ws).await?;
        let login = self.login(&b.host).await?;
        // The description: at every share of a team workspace, and the first share.
        let describe = match scope {
            ShareScope::Workspace => true,
            ShareScope::All => row.state == "team",
            ShareScope::Results => false,
        };
        if describe {
            let file = self.render_description(ws, b, &row, &login, true).await?;
            let bytes = files::render(&file);
            let remote = tip.files.get(files::DESCRIPTION_FILE);
            let record = records.get(files::DESCRIPTION_FILE);
            let add = remote.is_none() && record.is_none();
            let same = remote
                .is_some_and(|o| Oid::hash_object(git2::ObjectType::Blob, &bytes).ok() == Some(*o));
            let update = !same
                && remote
                    .is_some_and(|o| record.is_some_and(|(_, blob, _)| *blob == o.to_string()));
            if add || update {
                plan.workspace_subjects = changed_subjects(row.synced.as_ref(), &file);
                plan.changes
                    .insert(files::DESCRIPTION_FILE.to_owned(), Some(bytes));
                plan.description_changed = true;
            }
        }
        if scope == ShareScope::Workspace {
            return Ok(());
        }
        // Activity entries of this computer not yet shared.
        let rows = sqlx::query("SELECT * FROM team_activity WHERE workspace_id = ?1 AND mine = 1 AND blob IS NULL ORDER BY at, id")
            .bind(ws.to_string())
            .fetch_all(self.pool()?)
            .await?;
        for r in rows {
            let file = activity_file_of(&r, Some(&login))?;
            plan.changes.insert(
                format!("activity/{}.json", file.id),
                Some(files::render(&file)),
            );
            plan.activity += 1;
        }
        // Results, when the team folder keeps them and this person may add to it.
        if row.results_kind == "team_folder" && b.access == TeamAccess::ReadWrite {
            let unsent = self
                .deps
                .workspaces
                .unsent_results(ws)
                .await
                .unwrap_or_default();
            let pool = self.pool()?.clone();
            for r in unsent {
                if !files::result_path_ok(&r.path) {
                    continue;
                }
                let path = format!("results/{}", r.path);
                if r.size_bytes > files::TEAM_RESULT_MAX_BYTES {
                    store::put_problem(&pool, ws, &path, "TEAM_WS_RESULT_TOO_LARGE", "").await?;
                    continue;
                }
                let remote = tip.files.get(&path);
                let record = records.get(&path);
                // A colleague's newer file at this path is taken by a get first (TW-G step 5).
                let ours = remote.is_none()
                    || remote
                        .is_some_and(|o| record.is_some_and(|(_, blob, _)| *blob == o.to_string()));
                if !ours {
                    continue;
                }
                let Ok(bytes) = self.deps.workspaces.read_result(ws, &r.path).await else {
                    continue;
                };
                if bytes.len() as u64 > files::TEAM_RESULT_MAX_BYTES {
                    store::put_problem(&pool, ws, &path, "TEAM_WS_RESULT_TOO_LARGE", "").await?;
                    continue;
                }
                if remote.is_some_and(|o| {
                    Oid::hash_object(git2::ObjectType::Blob, &bytes).ok() == Some(*o)
                }) {
                    continue;
                }
                plan.results
                    .push(r.path.rsplit('/').next().unwrap_or(&r.path).to_owned());
                plan.changes.insert(path, Some(bytes));
            }
        }
        Ok(())
    }

    // ---- after the send (§G with TW-A step 7, TW-F step 3, TW-G step 4) ----

    pub(crate) async fn team_share_sent(
        &self,
        share: &store::Share,
        blobs: &HashMap<String, Option<Vec<u8>>>,
    ) -> Result<(), TeamError> {
        let ws = share.workspace_id;
        let Some(row) = self.team_row(ws).await? else {
            return Ok(());
        };
        let pool = self.pool()?.clone();
        let mut sent_results = Vec::new();
        for p in &share.planned {
            let Some(blob) = &p.blob else {
                continue;
            };
            let bytes = blobs.get(&p.path).cloned().flatten();
            match files::classify(&p.path) {
                Some(PathKind::Description) => {
                    self.put_file_record(ws, &p.path, "description", blob, None)
                        .await?;
                    if let Some(file) = bytes
                        .as_deref()
                        .and_then(|b| files::parse_description(b).ok())
                    {
                        self.set_synced(ws, &file).await?;
                    }
                }
                Some(PathKind::Activity { stem }) => {
                    self.put_file_record(ws, &p.path, "activity", blob, None)
                        .await?;
                    sqlx::query("UPDATE team_activity SET blob = ?2, shared_at = ?3, by_login = COALESCE(by_login, ?4) WHERE id = ?1")
                        .bind(&stem)
                        .bind(blob)
                        .bind(store::now())
                        .bind(bytes.as_deref().and_then(|b| files::parse_activity(b, &stem).ok()).map(|f| f.by))
                        .execute(&pool)
                        .await?;
                }
                Some(PathKind::Result { path }) => {
                    let sha = bytes.as_deref().map(sha256_hex).unwrap_or_default();
                    self.put_file_record(ws, &p.path, "result", blob, Some(&sha))
                        .await?;
                    store::clear_problem(&pool, ws, &p.path).await?;
                    sent_results.push((path, ordin_types::num::Sha256Hex(sha)));
                }
                _ => {}
            }
        }
        if !sent_results.is_empty() {
            let n = sent_results.len();
            if let Err(e) = self
                .deps
                .workspaces
                .mark_results_sent(ws, sent_results)
                .await
            {
                tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code(), "team.results_mark_failed");
            }
            store::clear_problems_with(&pool, ws, "TEAM_WS_RESULT_KEPT").await?;
            tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), results = n, "team.results_shared");
        }
        if row.state == "proposed" {
            self.become_team(ws).await?;
        }
        Ok(())
    }

    /// TW-A step 7 for a share that waits for review.
    pub(crate) async fn team_share_waiting(&self, share: &store::Share) -> Result<(), TeamError> {
        if share.scope != ShareScope::Workspace {
            return Ok(());
        }
        let ws = share.workspace_id;
        if self
            .team_row(ws)
            .await?
            .is_some_and(|r| r.state == "proposed")
        {
            let wanted: Vec<(String, Oid, u64)> = share
                .planned
                .iter()
                .filter(|p| p.path == files::DESCRIPTION_FILE)
                .filter_map(|p| {
                    Some((
                        p.path.clone(),
                        Oid::from_str(p.blob.as_deref()?).ok()?,
                        files::DESCRIPTION_MAX_BYTES as u64,
                    ))
                })
                .collect();
            let blobs = self.read_blobs(self.clone_path(ws), wanted).await?;
            if let Some(file) = blobs
                .get(files::DESCRIPTION_FILE)
                .cloned()
                .flatten()
                .and_then(|b| files::parse_description(&b).ok())
            {
                self.set_synced(ws, &file).await?;
            }
            self.become_team(ws).await?;
        }
        Ok(())
    }

    async fn become_team(&self, ws: WorkspaceId) -> Result<(), TeamError> {
        sqlx::query("UPDATE team_workspaces SET state = 'team' WHERE workspace_id = ?1")
            .bind(ws.to_string())
            .execute(self.pool()?)
            .await?;
        let places = self.place_rows(ws).await?.len();
        let agents = self.agent_rows(ws).await?.len();
        self.journal(
            "workspace_shared",
            ws,
            "This workspace was shared as a team workspace.",
            serde_json::json!({ "workspace_id": ws, "places": places, "agents": agents }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), places, agents, "team.workspace_shared");
        self.changed(ws).await;
        Ok(())
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-A
    /// TW-A step 8: a rejected first share undoes the team workspace.
    pub(crate) async fn team_share_rejected(&self, share: &store::Share) {
        if share.scope != ShareScope::Workspace {
            return;
        }
        let ws = share.workspace_id;
        let Ok(Some(row)) = self.team_row(ws).await else {
            return;
        };
        if row.state != "proposed" {
            return;
        }
        if let Ok(Some(folder)) = self.deps.workspaces.results_folder(ws).await {
            if let Ok(grants) = self.deps.workspaces.grants(ws).await {
                for g in grants.into_iter().filter(|g| g.place_id == folder) {
                    let _ = self
                        .deps
                        .workspaces
                        .set_grant(ws, g.grantee, folder, None)
                        .await;
                }
            }
        }
        let _ = self.delete_team_rows(ws).await;
        if row.turned_on_here {
            if let Err(e) = self.turn_off_locked(ws, "person").await {
                tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.turn_off_failed");
            }
        }
    }

    pub(crate) async fn team_share_failed(&self, _share: &store::Share, _code: &str) {
        // TW-A step 8: the row and the grants stay; the status carries `last_error`.
    }

    // ---- TW-C open ----

    /// TW-C steps 1–2: the description read with a clone of its own, which is removed.
    async fn read_open(
        &self,
        url: &str,
        branch: &str,
        path: &str,
    ) -> Result<
        (
            String,
            String,
            String,
            String,
            TeamAccess,
            TeamWorkspaceFile,
            Tip,
            Vec<(String, Oid)>,
            std::path::PathBuf,
        ),
        TeamError,
    > {
        let unsuitable = || TeamError::permanent("TEAM_LINK_NOT_SUITABLE");
        let (host, repository) = split_url(url).ok_or_else(unsuitable)?;
        if branch.is_empty()
            || branch.len() > 255
            || path.len() > 1_024
            || path.split('/').any(|s| s == "..")
            || path.starts_with('/')
        {
            return Err(unsuitable());
        }
        let folder = path.trim_end_matches('/').to_owned();
        let access = self.access_of(&host, &repository).await?;
        let temp = self.deps.clones_dir.join(format!("open-{}.git", uuid7()));
        let read = async {
            let tip = self.fetch(temp.clone(), &host, &repository, branch).await?;
            let tip = self.tip_files(temp.clone(), tip, &folder).await?;
            let mut wanted = Vec::new();
            if let Some(o) = tip.files.get(files::FORMAT_FILE) {
                wanted.push((
                    files::FORMAT_FILE.to_owned(),
                    *o,
                    files::ITEM_MAX_BYTES as u64,
                ));
            }
            let Some(desc) = tip.files.get(files::DESCRIPTION_FILE) else {
                return Err(TeamError::permanent("TEAM_WS_NOT_A_TEAM_WORKSPACE"));
            };
            wanted.push((
                files::DESCRIPTION_FILE.to_owned(),
                *desc,
                files::DESCRIPTION_MAX_BYTES as u64,
            ));
            let blobs = self.read_blobs(temp.clone(), wanted).await?;
            if let Some(f) = blobs.get(files::FORMAT_FILE) {
                if let Ok(Some(n)) = files::read_format(f.as_deref()) {
                    if n > files::FOLDER_FORMAT {
                        return Err(TeamError::permanent("TEAM_FORMAT_NEWER"));
                    }
                }
            }
            let bytes = blobs
                .get(files::DESCRIPTION_FILE)
                .cloned()
                .flatten()
                .ok_or_else(|| TeamError::permanent("TEAM_WS_DESCRIPTION_UNREADABLE"))?;
            let file = files::parse_description(&bytes).map_err(|e| match e {
                Some(_) => TeamError::permanent("TEAM_WS_FORMAT_NEWER"),
                None => TeamError::permanent("TEAM_WS_DESCRIPTION_UNREADABLE"),
            })?;
            let history = self.activity_paths(&tip);
            Ok((file, tip, history))
        }
        .await;
        match read {
            Ok((file, tip, history)) => Ok((
                host,
                repository,
                branch.to_owned(),
                folder,
                access,
                file,
                tip,
                history,
                temp,
            )),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&temp);
                Err(e)
            }
        }
    }

    fn activity_paths(&self, tip: &Tip) -> Vec<(String, Oid)> {
        tip.files
            .iter()
            .filter(|(p, _)| matches!(files::classify(p), Some(PathKind::Activity { .. })))
            .map(|(p, o)| (p.clone(), *o))
            .collect()
    }

    /// TW-C step 2's last refusal: the same folder or the same workspace key is open here.
    async fn already_open(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
        folder: &str,
        key: &str,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?;
        for b in store::bindings(pool).await? {
            let same = b.host.eq_ignore_ascii_case(host)
                && b.repository.eq_ignore_ascii_case(repository)
                && b.branch == branch
                && b.folder == folder;
            let keyed = self
                .team_row(b.workspace_id)
                .await?
                .is_some_and(|r| r.workspace_key == key);
            if same || keyed {
                if let Ok(w) = self.deps.workspaces.get(b.workspace_id).await {
                    if w.state != ordin_types::domain::WorkspaceState::InBin {
                        return Err(TeamError::permanent("TEAM_WS_ALREADY_OPEN"));
                    }
                }
            }
        }
        Ok(())
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-C
    pub async fn workspace_open_preview(
        &self,
        url: String,
        branch: String,
        path: String,
    ) -> Result<TeamWorkspaceOpenPreview, TeamError> {
        let (host, repository, branch, folder, _access, file, tip, _, temp) =
            self.read_open(&url, &branch, &path).await?;
        let _ = std::fs::remove_dir_all(&temp);
        self.already_open(&host, &repository, &branch, &folder, &file.workspace_key)
            .await?;
        let mut places = Vec::new();
        for p in &file.places {
            let kind = files::place_kind(&p.kind).unwrap_or(SourceLinkKind::SharepointLocation);
            let reach = self.reach_of(kind, &p.location).await;
            places.push(TeamPlaceLine {
                key: p.key.clone(),
                display_name: p.display_name.clone(),
                kind,
                reach: Some(reach),
            });
        }
        let mut agents = Vec::new();
        let ws_probe = WorkspaceId::new();
        for a in &file.agents {
            let found = match self
                .deps
                .agents
                .find_agent(ws_probe, &a.repository, &a.id, &a.version)
                .await
            {
                Ok(TeamAgentFind::Present { .. } | TeamAgentFind::OtherVersion { .. }) => {
                    TeamAgentFound::AlreadyHere
                }
                Ok(TeamAgentFind::Available) => TeamAgentFound::InYourLibrary,
                _ => TeamAgentFound::NotInYourRepositories,
            };
            agents.push(TeamAgentLine {
                key: a.key.clone(),
                name: a.name.clone(),
                access: access_lines(&file, a),
                found: Some(found),
            });
        }
        let mut knowledge = TeamCounts::default();
        for p in tip.files.keys() {
            match files::classify(p) {
                Some(PathKind::Item {
                    kind: files::ItemKind::Fact,
                    ..
                }) => knowledge.new_facts += 1,
                Some(PathKind::Item {
                    kind: files::ItemKind::Page,
                    ..
                }) => knowledge.new_pages += 1,
                _ => {}
            }
        }
        let header = if folder.is_empty() {
            repository.clone()
        } else {
            format!("{repository} › {folder}")
        };
        Ok(TeamWorkspaceOpenPreview {
            // SPEC-GAP-4: `name_in_use` needs the names of this computer's workspaces, which the
            // `TeamWorkspaces` trait cannot list; it is false here and `open_workspace` refuses a taken
            // name with TEAM_WS_NAME_IN_USE from `create`.
            name_in_use: false,
            name: file.name.clone(),
            folder_name: header.clone(),
            places,
            agents,
            knowledge,
            results: results_name(&file, &header),
        })
    }

    /// TW-C step 3's reach of a place.
    async fn reach_of(&self, kind: SourceLinkKind, location: &str) -> TeamReach {
        if kind == SourceLinkKind::GitRemote {
            return match parse_origin(location) {
                Some((h, r, _, _)) => match self.access_of(&h, &r).await {
                    Ok(_) => TeamReach::WillLink,
                    Err(e) if e.code == "TEAM_NO_ACCESS" => TeamReach::NoAccess,
                    Err(e) if e.code == "TEAM_SIGN_IN_NEEDED" => TeamReach::SignInNeeded,
                    Err(_) => TeamReach::CannotTell,
                },
                None => TeamReach::CannotTell,
            };
        }
        match self.deps.workspaces.check_origin(kind, location).await {
            Ok(r) => match r.reach {
                Reach::Reachable => TeamReach::WillLink,
                Reach::NoAccess => TeamReach::NoAccess,
                Reach::SignInNeeded => TeamReach::SignInNeeded,
                Reach::NotConfigured => TeamReach::NotSetUp,
                Reach::Unavailable => TeamReach::CannotTell,
            },
            Err(_) => TeamReach::CannotTell,
        }
    }

    pub async fn open_workspace(
        &self,
        url: String,
        branch: String,
        path: String,
        name: String,
    ) -> Result<TeamWorkspaceOpened, TeamError> {
        let name = name.trim().to_owned();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(TeamError::permanent("TEAM_WS_NAME_IN_USE"));
        }
        let (host, repository, branch, folder, _access, file, tip, activity, temp) =
            self.read_open(&url, &branch, &path).await?;
        // TW-J on the names the file gives, from the preview's clone.
        let checks = description_checks(&file, None);
        let confirmed = self
            .confirm_names(
                temp.clone(),
                tip.commit,
                &folder,
                checks.clone(),
                Vec::new(),
            )
            .await;
        let _ = activity;
        let _ = std::fs::remove_dir_all(&temp);
        self.already_open(&host, &repository, &branch, &folder, &file.workspace_key)
            .await?;
        let workspace = match self.deps.workspaces.create(name).await {
            Ok(w) => w,
            Err(ref e) if e.code() == "WS_NAME_DUPLICATE" => {
                return Err(TeamError::permanent("TEAM_WS_NAME_IN_USE"))
            }
            Err(e) => return Err(ws_error(&e, "TEAM_WS_OPEN_FAILED")),
        };
        let ws = workspace.id;
        let guard = self.workspace_lock(ws);
        let held = guard.lock().await;
        let choice = TeamFolderChoice::NewFolder { url, branch, path };
        if let Err(e) = self.turn_on_locked(ws, &choice).await {
            let _ = self.deps.workspaces.move_to_bin(ws).await;
            let _ = self.deps.workspaces.delete_from_bin(ws).await;
            return Err(e);
        }
        let opened = self.open_steps(ws, &file, &confirmed).await;
        drop(held);
        opened?;
        lock_set_busy(self, ws);
        let status = self.status(ws).await?;
        if let Some(me) = self.me.upgrade() {
            tokio::spawn(async move {
                let guard = me.workspace_lock(ws);
                let held = guard.lock().await;
                let outcome = me.get_now(ws).await.map(|_| ());
                drop(held);
                me.finish_long(ws, outcome).await;
            });
        }
        Ok(TeamWorkspaceOpened {
            workspace_id: ws,
            status,
        })
    }

    /// TW-C steps 4.2 to 4.6.
    async fn open_steps(
        &self,
        ws: WorkspaceId,
        file: &TeamWorkspaceFile,
        confirmed: &[bool],
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        sqlx::query(
            "INSERT INTO team_workspaces (workspace_id, state, workspace_key, turned_on_here, synced_description, results_kind, results_place_key, created_at) \
             VALUES (?1, 'team', ?2, 1, ?3, ?4, ?5, ?6)",
        )
        .bind(ws.to_string())
        .bind(&file.workspace_key)
        .bind(store::json(file))
        .bind(&file.results.kind)
        .bind(&file.results.place)
        .bind(store::now())
        .execute(&pool)
        .await?;
        let _ = confirmed;
        let mut linked = 0u32;
        let mut place_ids: HashMap<String, PlaceId> = HashMap::new();
        for p in &file.places {
            let kind = files::place_kind(&p.kind).unwrap_or(SourceLinkKind::SharepointLocation);
            let state = match self
                .deps
                .workspaces
                .add_link_from_origin(ws, kind, &p.location, &p.display_name)
                .await
            {
                Ok(link) => {
                    linked += 1;
                    if let Some(pid) = link.place_id {
                        place_ids.insert(p.key.clone(), pid);
                    }
                    self.put_place_row(ws, &p.key, Some(link.id), "linked")
                        .await?;
                    continue;
                }
                Err(ref e) if e.code() == "WS_SOURCE_UNAVAILABLE" => "waiting",
                Err(_) => "no_access",
            };
            self.put_place_row(ws, &p.key, None, state).await?;
        }
        let results = self
            .deps
            .workspaces
            .make_results_folder(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WS_OPEN_FAILED"))?;
        let workspace = self.active_workspace(ws).await?;
        let mut added = 0u32;
        for a in &file.agents {
            let instance = match self
                .deps
                .agents
                .find_agent(ws, &a.repository, &a.id, &a.version)
                .await
            {
                Ok(
                    TeamAgentFind::Present { agent_instance_id }
                    | TeamAgentFind::OtherVersion {
                        agent_instance_id, ..
                    },
                ) => Some(agent_instance_id),
                Ok(TeamAgentFind::Available) => match self
                    .deps
                    .agents
                    .add_agent(ws, &a.repository, &a.id, &a.version)
                    .await
                {
                    Ok(id) => {
                        added += 1;
                        Some(id)
                    }
                    Err(e) => {
                        tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.agent_not_added");
                        self.put_agent_row(ws, &a.key, None, "not_found", Some(e.code))
                            .await?;
                        None
                    }
                },
                _ => {
                    self.put_agent_row(ws, &a.key, None, "not_found", None)
                        .await?;
                    None
                }
            };
            let Some(instance) = instance else {
                continue;
            };
            self.put_agent_row(ws, &a.key, Some(instance), "present", None)
                .await?;
            for e in &a.access {
                let place = match e.place.as_str() {
                    "own" => Some(workspace.own_folder_place_id),
                    "results" => Some(results.id),
                    key => place_ids.get(key).copied(),
                };
                if let Some(place) = place {
                    let grantee = Grantee::AgentInstance {
                        agent_instance_id: instance,
                    };
                    self.deps
                        .workspaces
                        .set_grant(ws, grantee, place, Some(access_of(&e.access)))
                        .await
                        .map_err(|e| ws_error(&e, "TEAM_WS_OPEN_FAILED"))?;
                }
            }
        }
        // The description's files are recorded as synced: they came with the open.
        self.journal(
            "workspace_opened",
            ws,
            "A team workspace was opened on this computer.",
            serde_json::json!({ "workspace_id": ws, "places": file.places.len(), "linked": linked, "agents": file.agents.len(), "added": added }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), linked, left_out = file.places.len() as u32 - linked, agents = file.agents.len(), added, "team.workspace_opened");
        self.changed(ws).await;
        Ok(())
    }

    // ---- TW-D, TW-F step 4, TW-G step 5: what a get takes in ----

    pub(crate) async fn get_team_parts(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        tip: &Tip,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let records = self.file_records(ws).await?;
        let path = self.clone_path(ws);

        // Activity files not recorded yet, oldest first, at most 5,000.
        let mut new_activity: Vec<(String, String, Oid)> = tip
            .files
            .iter()
            .filter_map(|(p, o)| match files::classify(p) {
                Some(PathKind::Activity { stem }) if !records.contains_key(p) => {
                    Some((p.clone(), stem, *o))
                }
                _ => None,
            })
            .collect();
        new_activity.sort_by(|a, b| a.1.cmp(&b.1));
        new_activity.truncate(MAX_PER_GET);
        // Records whose file left the folder.
        for (p, (kind, _, _)) in &records {
            if kind == "activity" && !tip.files.contains_key(p) {
                if let Some(PathKind::Activity { stem }) = files::classify(p) {
                    sqlx::query("DELETE FROM team_activity WHERE id = ?1 AND workspace_id = ?2 AND mine = 0").bind(&stem).bind(ws.to_string()).execute(&pool).await?;
                }
                self.drop_file_record(ws, p).await?;
            }
        }
        let wanted = new_activity
            .iter()
            .map(|(p, _, o)| (p.clone(), *o, files::ACTIVITY_MAX_BYTES as u64))
            .collect();
        let blobs = self.read_blobs(path.clone(), wanted).await?;
        let mut activity: Vec<(String, TeamActivityFile, Oid)> = Vec::new();
        for (p, stem, o) in &new_activity {
            match blobs
                .get(p)
                .cloned()
                .flatten()
                .as_deref()
                .map(|b| files::parse_activity(b, stem))
            {
                Some(Ok(f)) => activity.push((p.clone(), f, *o)),
                _ => {
                    store::put_problem(&pool, ws, p, "TEAM_FILE_UNREADABLE", &o.to_string()).await?
                }
            }
        }

        // The description.
        let row = self.team_row(ws).await?;
        let mut taken_settings: Vec<(Setting, String, Timestamp)> = Vec::new();
        let mut merge: Option<(TeamWorkspaceFile, TeamWorkspaceFile)> = None;
        if let (Some(row), Some(oid)) = (row.as_ref(), tip.files.get(files::DESCRIPTION_FILE)) {
            let unchanged = records
                .get(files::DESCRIPTION_FILE)
                .is_some_and(|(_, blob, _)| *blob == oid.to_string());
            if !unchanged && row.state == "team" {
                let blobs = self
                    .read_blobs(
                        path.clone(),
                        vec![(
                            files::DESCRIPTION_FILE.to_owned(),
                            *oid,
                            files::DESCRIPTION_MAX_BYTES as u64,
                        )],
                    )
                    .await?;
                match blobs
                    .get(files::DESCRIPTION_FILE)
                    .cloned()
                    .flatten()
                    .map(|b| files::parse_description(&b))
                {
                    Some(Ok(remote)) => {
                        let login = self.login(&b.host).await?;
                        let local = self.render_description(ws, b, row, &login, false).await?;
                        let synced = row.synced.clone().unwrap_or_else(|| remote.clone());
                        for (s, by, at) in settings_of(&remote) {
                            if !setting_holds(&synced, &s, &by, at) {
                                taken_settings.push((s, by, at));
                            }
                        }
                        merge = Some((remote, local));
                        store::clear_problem(&pool, ws, files::DESCRIPTION_FILE).await?;
                    }
                    Some(Err(Some(_))) => {
                        store::put_problem(
                            &pool,
                            ws,
                            files::DESCRIPTION_FILE,
                            "TEAM_WS_FORMAT_NEWER",
                            &oid.to_string(),
                        )
                        .await?
                    }
                    _ => {
                        store::put_problem(
                            &pool,
                            ws,
                            files::DESCRIPTION_FILE,
                            "TEAM_WS_DESCRIPTION_UNREADABLE",
                            &oid.to_string(),
                        )
                        .await?
                    }
                }
            }
        }

        // TW-J: one history walk for every name of this get.
        let setting_checks: Vec<NameCheck> = taken_settings
            .iter()
            .map(|(s, by, at)| NameCheck::Setting(s.clone(), by.clone(), *at))
            .collect();
        let activity_checks: Vec<(String, String)> = activity
            .iter()
            .map(|(p, f, _)| {
                (
                    p.clone(),
                    if f.kind == "replaced" {
                        f.replaced_by.clone().unwrap_or_default()
                    } else {
                        f.by.clone()
                    },
                )
            })
            .collect();
        let confirmed = self
            .confirm_names(
                path.clone(),
                tip.commit,
                &b.folder,
                setting_checks.clone(),
                activity_checks,
            )
            .await;
        let unconfirmed = confirmed.iter().filter(|c| !**c).count();
        if unconfirmed > 0 {
            tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), count = unconfirmed, "team.names_unconfirmed");
        }
        let (setting_ok, activity_ok) =
            confirmed.split_at(setting_checks.len().min(confirmed.len()));

        for ((p, f, o), ok) in activity
            .iter()
            .zip(activity_ok.iter().chain(std::iter::repeat(&false)))
        {
            let (by_confirmed, replaced_confirmed) = if f.kind == "replaced" {
                (false, *ok)
            } else {
                (*ok, false)
            };
            sqlx::query(
                "INSERT INTO team_activity (id, workspace_id, mine, kind, by_login, by_confirmed, at, task, agent, outcome, results, change, subject, replaced_by, replaced_by_confirmed, run_id, blob, shared_at) \
                 VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16) ON CONFLICT (id) DO NOTHING",
            )
            .bind(f.id.to_string())
            .bind(ws.to_string())
            .bind(&f.kind)
            .bind(&f.by)
            .bind(i64::from(by_confirmed))
            .bind(f.at.to_wire())
            .bind(&f.task)
            .bind(&f.agent)
            .bind(&f.outcome)
            .bind(store::json(&f.results))
            .bind(&f.change)
            .bind(&f.subject)
            .bind(&f.replaced_by)
            .bind(i64::from(replaced_confirmed))
            .bind(o.to_string())
            .bind(store::now())
            .execute(&pool)
            .await?;
            self.put_file_record(ws, p, "activity", &o.to_string(), None)
                .await?;
        }

        if let (Some((remote, local)), Some(row)) = (merge, row.as_ref()) {
            let confirmations: HashMap<Setting, bool> = setting_checks
                .iter()
                .zip(setting_ok.iter())
                .map(|(c, ok)| match c {
                    NameCheck::Setting(s, _, _) => (s.clone(), *ok),
                })
                .collect();
            self.merge_description(ws, b, row, remote, local, &confirmations)
                .await?;
            if let Some(oid) = tip.files.get(files::DESCRIPTION_FILE) {
                self.put_file_record(
                    ws,
                    files::DESCRIPTION_FILE,
                    "description",
                    &oid.to_string(),
                    None,
                )
                .await?;
            }
        }

        // Results (TW-G step 5).
        let row = self.team_row(ws).await?;
        if row
            .as_ref()
            .is_some_and(|r| r.results_kind == "team_folder")
        {
            self.take_results(ws, b, tip, &records).await?;
        }
        Ok(())
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-D
    // SPEC-GAP-5: TW-D step 2 keeps S for a setting both people changed, so every later get would see
    // R ≠ S and L ≠ S again and record the same `replaced` entry until this person shares. The new
    // synced description takes R's setting in that case too: the replaced entry is written once, and
    // L still differs from S, so this person's next share sends it with a new stamp.
    async fn merge_description(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        row: &TeamRow,
        remote: TeamWorkspaceFile,
        local: TeamWorkspaceFile,
        confirmations: &HashMap<Setting, bool>,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let synced = row.synced.clone().unwrap_or_else(|| remote.clone());
        let mut next = synced.clone();
        let mut taken = 0u32;
        let mut kept = 0u32;
        let login = self.login(&b.host).await.unwrap_or_default();
        let mut ids: BTreeSet<Setting> = BTreeSet::new();
        for f in [&synced, &remote, &local] {
            ids.extend(settings_of(f).into_iter().map(|(s, _, _)| s));
        }
        let mut waiting_grants = 0u32;
        for id in ids {
            let s = setting_value(&synced, &id);
            let r = setting_value(&remote, &id);
            let l = setting_value(&local, &id);
            let r_full = setting_full(&remote, &id);
            let s_full = setting_full(&synced, &id);
            if r_full == s_full {
                continue;
            }
            if l == s {
                // Take R.
                self.apply_setting(
                    ws,
                    b,
                    &id,
                    &synced,
                    &remote,
                    confirmations.get(&id).copied().unwrap_or(false),
                    &mut waiting_grants,
                )
                .await?;
                set_setting(&mut next, &remote, &id);
                taken += 1;
            } else if l != r {
                // The later share wins: L is kept and the colleague's change is listed as replaced.
                let (by, subject) = setting_who(&remote, &synced, &id);
                if files::login_ok(&by) && files::login_ok(&login) {
                    let id_text = TeamActivityId::new().to_string();
                    sqlx::query(
                        "INSERT INTO team_activity (id, workspace_id, mine, kind, by_login, by_confirmed, at, task, agent, outcome, results, change, subject, replaced_by, replaced_by_confirmed, run_id, blob, shared_at) \
                         VALUES (?1, ?2, 1, 'replaced', ?3, 0, ?4, NULL, NULL, NULL, '[]', ?5, ?6, ?7, 1, NULL, NULL, NULL)",
                    )
                    .bind(&id_text)
                    .bind(ws.to_string())
                    .bind(&by)
                    .bind(store::now())
                    .bind(change_name(&id, &synced, &remote))
                    .bind(subject.chars().take(120).collect::<String>())
                    .bind(&login)
                    .execute(&pool)
                    .await?;
                }
                set_setting(&mut next, &remote, &id);
                kept += 1;
            } else {
                set_setting(&mut next, &remote, &id);
            }
        }
        self.set_synced(ws, &next).await?;
        self.journal("workspace_got", ws, "Your team's changes to this workspace were taken in.", serde_json::json!({ "workspace_id": ws, "taken": taken, "kept": kept, "waiting_grants": waiting_grants })).await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), taken, kept, waiting_grants, "team.workspace_got");
        if taken > 0 {
            self.changed(ws).await;
        }
        Ok(())
    }

    /// TW-D step 3.
    #[allow(clippy::too_many_arguments)]
    async fn apply_setting(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        id: &Setting,
        synced: &TeamWorkspaceFile,
        remote: &TeamWorkspaceFile,
        confirmed: bool,
        waiting: &mut u32,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        match id {
            // SPEC-GAP-10: TW-D step 3 records `TEAM_WS_NAME_IN_USE` and `TEAM_WS_RESULTS_NO_ACCESS` as
            // problems without a path, and a problem row is keyed by path; they are kept at
            // `ordin-team-workspace.json#name` and `ordin-team-workspace.json#results`.
            Setting::Name => match self.deps.workspaces.rename(ws, remote.name.clone()).await {
                Ok(_) => store::clear_problem(&pool, ws, "ordin-team-workspace.json#name").await?,
                Err(ref e) if e.code() == "WS_NAME_DUPLICATE" => {
                    store::put_problem(
                        &pool,
                        ws,
                        "ordin-team-workspace.json#name",
                        "TEAM_WS_NAME_IN_USE",
                        "",
                    )
                    .await?;
                }
                Err(e) => {
                    tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code(), "team.rename_failed")
                }
            },
            Setting::Place(key) => {
                let rows = self.place_rows(ws).await?;
                let row = rows.iter().find(|(k, _, _)| k == key);
                match (
                    synced.places.iter().find(|p| &p.key == key),
                    remote.places.iter().find(|p| &p.key == key),
                ) {
                    (_, Some(p)) if row.is_none_or(|(_, l, _)| l.is_none()) => {
                        let kind = files::place_kind(&p.kind)
                            .unwrap_or(SourceLinkKind::SharepointLocation);
                        match self
                            .deps
                            .workspaces
                            .add_link_from_origin(ws, kind, &p.location, &p.display_name)
                            .await
                        {
                            Ok(link) => {
                                self.put_place_row(ws, key, Some(link.id), "linked").await?
                            }
                            Err(ref e) if e.code() == "WS_SOURCE_UNAVAILABLE" => {
                                self.put_place_row(ws, key, None, "waiting").await?
                            }
                            Err(_) => self.put_place_row(ws, key, None, "no_access").await?,
                        }
                    }
                    (Some(_), None) => {
                        if let Some((_, Some(link), _)) = row {
                            let items = self
                                .deps
                                .workspaces
                                .working_copy_items(*link)
                                .await
                                .unwrap_or_default();
                            let unsent = items.iter().any(|i| {
                                matches!(
                                    i.state,
                                    WorkingCopyState::Changed
                                        | WorkingCopyState::AwaitingPush
                                        | WorkingCopyState::OriginChanged
                                )
                            });
                            if unsent {
                                self.put_place_row(ws, key, Some(*link), "removal_waits")
                                    .await?;
                                return Ok(());
                            }
                            let _ = self.deps.workspaces.remove_link(*link).await;
                        }
                        sqlx::query("DELETE FROM team_workspace_places WHERE workspace_id = ?1 AND key = ?2").bind(ws.to_string()).bind(key).execute(&pool).await?;
                    }
                    // A changed display name alone is stored with the synced description.
                    _ => {}
                }
            }
            Setting::Agent(key) => {
                let rows = self.agent_rows(ws).await?;
                let here = rows
                    .iter()
                    .find(|(k, _, _, _)| k == key)
                    .and_then(|(_, a, st, _)| (st == "present").then_some(*a).flatten());
                let declined_at = rows
                    .iter()
                    .find(|(k, _, _, _)| k == key)
                    .and_then(|(_, _, _, d)| *d);
                let before = synced.agents.iter().find(|a| &a.key == key);
                match remote.agents.iter().find(|a| &a.key == key) {
                    None => {
                        if let Some(agent) = here {
                            let _ = self.deps.agents.remove_agent(ws, agent).await;
                        }
                        self.decline_waiting_for(ws, key).await?;
                        sqlx::query("DELETE FROM team_workspace_agents WHERE workspace_id = ?1 AND key = ?2").bind(ws.to_string()).bind(key).execute(&pool).await?;
                    }
                    Some(r) => {
                        let old: HashMap<&str, &str> = before
                            .map(|a| {
                                a.access
                                    .iter()
                                    .map(|e| (e.place.as_str(), e.access.as_str()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let raised: Vec<&TeamAccessEntry> = r
                            .access
                            .iter()
                            .filter(|e| {
                                here.is_none()
                                    || match old.get(e.place.as_str()) {
                                        None => true,
                                        Some(a) => *a == "read" && e.access == "read_and_change",
                                    }
                            })
                            .collect();
                        let new_places: BTreeSet<&str> =
                            r.access.iter().map(|e| e.place.as_str()).collect();
                        // Lowered or removed access applies now.
                        if let Some(agent) = here {
                            for (place, was) in &old {
                                let now = r.access.iter().find(|e| e.place == *place);
                                let lowered = match now {
                                    None => Some(None),
                                    Some(e) if *was == "read_and_change" && e.access == "read" => {
                                        Some(Some(FolderAccess::Read))
                                    }
                                    _ => None,
                                };
                                if let (Some(access), Some(pid)) =
                                    (lowered, self.place_id_of(ws, place).await?)
                                {
                                    let _ = self
                                        .deps
                                        .workspaces
                                        .set_grant(
                                            ws,
                                            Grantee::AgentInstance {
                                                agent_instance_id: agent,
                                            },
                                            pid,
                                            access,
                                        )
                                        .await;
                                }
                                let _ = &new_places;
                            }
                        }
                        let asked_again = declined_at.is_none_or(|d| r.changed_at > d);
                        if !raised.is_empty() && asked_again {
                            let mut access = Vec::new();
                            for e in raised {
                                access.push(PendingAccess {
                                    place_key: e.place.clone(),
                                    place_id: self.place_id_of(ws, &e.place).await?,
                                    place_name: self.place_name(ws, remote, &e.place).await,
                                    access: access_of(&e.access),
                                });
                            }
                            let grant = NewPendingGrant {
                                workspace_id: ws,
                                agent_key: key.clone(),
                                agent_name: r.name.clone(),
                                repository: r.repository.clone(),
                                agent_id: r.id.clone(),
                                agent_version: r.version.clone(),
                                agent_instance_id: here,
                                access,
                                added_by: r.changed_by.clone(),
                                added_by_confirmed: confirmed,
                                added_at: r.changed_at,
                            };
                            self.deps
                                .workspaces
                                .record_pending_grant(grant)
                                .await
                                .map_err(|e| ws_error(&e, "TEAM_WS_ALLOW_FAILED"))?;
                            *waiting += 1;
                            if here.is_none() {
                                self.put_agent_row(ws, key, None, "waiting", None).await?;
                            }
                        }
                    }
                }
            }
            Setting::Results => {
                self.set_results(ws, &remote.results.kind, remote.results.place.as_deref())
                    .await?;
                let blocked = match &remote.results.place {
                    Some(key) => self
                        .place_rows(ws)
                        .await?
                        .iter()
                        .any(|(k, _, st)| k == key && st == "no_access"),
                    None => false,
                };
                if blocked {
                    store::put_problem(
                        &pool,
                        ws,
                        "ordin-team-workspace.json#results",
                        "TEAM_WS_RESULTS_NO_ACCESS",
                        "",
                    )
                    .await?;
                } else {
                    store::clear_problem(&pool, ws, "ordin-team-workspace.json#results").await?;
                }
            }
        }
        let _ = b;
        Ok(())
    }

    async fn decline_waiting_for(&self, ws: WorkspaceId, key: &str) -> Result<(), TeamError> {
        if let Ok(grants) = self.deps.workspaces.pending_grants(ws).await {
            for g in grants.into_iter().filter(|g| g.grant.agent_key == key) {
                let _ = self
                    .deps
                    .workspaces
                    .decide_pending_grant(g.id, GrantDecision::Declined)
                    .await;
            }
        }
        Ok(())
    }

    /// The place a key names on this computer: a linked place, `own`, or `results`.
    async fn place_id_of(&self, ws: WorkspaceId, key: &str) -> Result<Option<PlaceId>, TeamError> {
        Ok(match key {
            "own" => self
                .deps
                .workspaces
                .get(ws)
                .await
                .ok()
                .map(|w| w.own_folder_place_id),
            "results" => self.deps.workspaces.results_folder(ws).await.ok().flatten(),
            key => {
                let rows = self.place_rows(ws).await?;
                let link = rows
                    .into_iter()
                    .find(|(k, _, st)| k == key && st != "no_access")
                    .and_then(|(_, l, _)| l);
                match link {
                    Some(l) => self
                        .deps
                        .workspaces
                        .source_links(ws)
                        .await
                        .ok()
                        .and_then(|ls| ls.into_iter().find(|x| x.id == l))
                        .and_then(|x| x.place_id),
                    None => None,
                }
            }
        })
    }

    // SPEC-GAP-6: a place name for the access keys `own` and `results` is not named by the spec; the
    // workspace's own name and the results folder's friendly name ("Team results") are shown.
    async fn place_name(&self, ws: WorkspaceId, file: &TeamWorkspaceFile, key: &str) -> String {
        match key {
            "own" => self
                .deps
                .workspaces
                .get(ws)
                .await
                .map(|w| w.name)
                .unwrap_or_default(),
            "results" => ordin_workspaces::TEAM_RESULTS_NAME.to_owned(),
            key => file
                .places
                .iter()
                .find(|p| p.key == key)
                .map(|p| p.display_name.clone())
                .unwrap_or_default(),
        }
    }

    /// TW-G step 5.
    async fn take_results(
        &self,
        ws: WorkspaceId,
        b: &Binding,
        tip: &Tip,
        records: &HashMap<String, (String, String, Option<String>)>,
    ) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        let unsent: BTreeMap<String, ordin_workspaces::TeamResultFile> = self
            .deps
            .workspaces
            .unsent_results(ws)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| (r.path.clone(), r))
            .collect();
        let mut changed: Vec<(String, String, Oid)> = Vec::new();
        for (p, o) in &tip.files {
            if let Some(PathKind::Result { path }) = files::classify(p) {
                let same = records
                    .get(p)
                    .is_some_and(|(_, blob, _)| *blob == o.to_string());
                if !same {
                    changed.push((p.clone(), path, *o));
                }
            }
        }
        changed.truncate(MAX_PER_GET);
        let mut both = 0u32;
        let login = self.login(&b.host).await.ok();
        for (p, rel, o) in changed {
            if !files::result_path_ok(&rel) {
                store::put_problem(&pool, ws, &p, "TEAM_FILE_UNREADABLE", &o.to_string()).await?;
                continue;
            }
            let blobs = self
                .read_blobs(
                    self.clone_path(ws),
                    vec![(p.clone(), o, files::TEAM_RESULT_MAX_BYTES)],
                )
                .await?;
            let Some(bytes) = blobs.get(&p).cloned().flatten() else {
                store::put_problem(&pool, ws, &p, "TEAM_FILE_UNREADABLE", &o.to_string()).await?;
                continue;
            };
            if unsent.contains_key(&rel) {
                // Both are kept (SA-TEAM-TW-12).
                let Some(login) = login.clone() else {
                    store::put_problem(&pool, ws, &p, "TEAM_WS_RESULT_NOT_TAKEN", &o.to_string())
                        .await?;
                    continue;
                };
                let mut kept = None;
                for n in 1..=99u32 {
                    let candidate = files::both_kept_name(&rel, &login, n);
                    let taken = unsent.contains_key(&candidate)
                        || tip.files.contains_key(&format!("results/{candidate}"));
                    if !files::result_path_ok(&candidate) || taken {
                        continue;
                    }
                    match self
                        .deps
                        .workspaces
                        .keep_both_results(ws, &rel, bytes.clone(), &candidate)
                        .await
                    {
                        Ok(sha) => {
                            kept = Some((candidate, sha));
                            break;
                        }
                        Err(ref e) if e.code() == "WS_RESULT_PATH_TAKEN" => continue,
                        Err(_) => break,
                    }
                }
                match kept {
                    Some((mine_to, sha)) => {
                        both += 1;
                        self.put_file_record(ws, &p, "result", &o.to_string(), Some(&sha.0))
                            .await?;
                        store::clear_problem(&pool, ws, &p).await?;
                        store::put_problem(
                            &pool,
                            ws,
                            &format!("results/{mine_to}"),
                            "TEAM_WS_RESULT_KEPT",
                            "",
                        )
                        .await?;
                        // This computer's entries not yet shared list the new name.
                        let rows = sqlx::query("SELECT id, results FROM team_activity WHERE workspace_id = ?1 AND mine = 1 AND blob IS NULL")
                            .bind(ws.to_string())
                            .fetch_all(&pool)
                            .await?;
                        for r in rows {
                            let id: String = r.try_get("id")?;
                            let list: String = r.try_get("results")?;
                            let mut paths: Vec<String> =
                                serde_json::from_str(&list).unwrap_or_default();
                            if paths.contains(&rel) {
                                for x in paths.iter_mut().filter(|x| **x == rel) {
                                    *x = mine_to.clone();
                                }
                                sqlx::query("UPDATE team_activity SET results = ?2 WHERE id = ?1")
                                    .bind(&id)
                                    .bind(store::json(&paths))
                                    .execute(&pool)
                                    .await?;
                            }
                        }
                    }
                    None => {
                        store::put_problem(
                            &pool,
                            ws,
                            &p,
                            "TEAM_WS_RESULT_NOT_TAKEN",
                            &o.to_string(),
                        )
                        .await?
                    }
                }
                continue;
            }
            match self
                .deps
                .workspaces
                .store_team_result(ws, &rel, bytes)
                .await
            {
                Ok(sha) => {
                    self.put_file_record(ws, &p, "result", &o.to_string(), Some(&sha.0))
                        .await?;
                    store::clear_problem(&pool, ws, &p).await?;
                }
                Err(ref e) if e.code() == "WS_RESULT_PATH_INVALID" => {
                    store::put_problem(&pool, ws, &p, "TEAM_FILE_UNREADABLE", &o.to_string())
                        .await?
                }
                Err(e) => {
                    tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code(), "team.result_not_stored")
                }
            }
        }
        if both > 0 {
            tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), count = both, "team.results_both_kept");
        }
        // Removed from the folder.
        for (p, (kind, _, _)) in records {
            if kind != "result" || tip.files.contains_key(p) {
                continue;
            }
            if let Some(PathKind::Result { path }) = files::classify(p) {
                if !unsent.contains_key(&path) {
                    let _ = self.deps.workspaces.remove_team_result(ws, &path).await;
                }
            }
            self.drop_file_record(ws, p).await?;
        }
        Ok(())
    }

    // ---- TW-J ----

    /// TW-J: for each name, whether the commit that wrote it has an author name ending ` (<login>)`.
    /// `settings` first, then `activity` (path, login), in the order given. A Git error → none agrees.
    async fn confirm_names(
        &self,
        clone: std::path::PathBuf,
        tip: Oid,
        folder: &str,
        settings: Vec<NameCheck>,
        activity: Vec<(String, String)>,
    ) -> Vec<bool> {
        let total = settings.len() + activity.len();
        if total == 0 {
            return Vec::new();
        }
        let folder = folder.to_owned();
        let result = tokio::task::spawn_blocking(move || -> Result<Vec<bool>, git::GitFailure> {
            let repo = git2::Repository::open_bare(&clone).map_err(|_| git::GitFailure::Disk)?;
            let history = git::history(&repo, tip, HISTORY_LIMIT)?;
            let mut descriptions: HashMap<Oid, Option<TeamWorkspaceFile>> = HashMap::new();
            let mut description_at = |commit: Oid| -> Option<TeamWorkspaceFile> {
                let blob = git::blob_at(&repo, commit, &folder, files::DESCRIPTION_FILE)?;
                descriptions
                    .entry(blob)
                    .or_insert_with(|| {
                        git::read_blob(&repo, blob, files::DESCRIPTION_MAX_BYTES as u64)
                            .ok()
                            .flatten()
                            .and_then(|b| files::parse_description(&b).ok())
                    })
                    .clone()
            };
            let agrees = |author: &str, login: &str| {
                author
                    .to_lowercase()
                    .ends_with(&format!(" ({})", login.to_lowercase()))
            };
            let mut out = Vec::with_capacity(total);
            for check in &settings {
                let NameCheck::Setting(s, by, at) = check;
                let mut found = false;
                for (commit, parents, author) in &history {
                    let holds =
                        description_at(*commit).is_some_and(|d| setting_holds(&d, s, by, *at));
                    if !holds {
                        continue;
                    }
                    let parent_holds = parents
                        .iter()
                        .any(|p| description_at(*p).is_some_and(|d| setting_holds(&d, s, by, *at)));
                    if !parent_holds {
                        found = agrees(author, by);
                        break;
                    }
                }
                out.push(found);
            }
            let repo2 = git2::Repository::open_bare(&clone).map_err(|_| git::GitFailure::Disk)?;
            for (path, login) in &activity {
                let mut found = false;
                for (commit, parents, author) in &history {
                    if git::blob_at(&repo2, *commit, &folder, path).is_none() {
                        continue;
                    }
                    if parents
                        .iter()
                        .all(|p| git::blob_at(&repo2, *p, &folder, path).is_none())
                    {
                        found = agrees(author, login);
                        break;
                    }
                }
                out.push(found);
            }
            Ok(out)
        })
        .await;
        match result {
            Ok(Ok(v)) => v,
            _ => {
                tracing::warn!(target: "ordin::team_knowledge", "team.history_unread");
                vec![false; total]
            }
        }
    }

    // ---- TW-E ----

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-E
    pub async fn pending_grants(
        &self,
        ws: WorkspaceId,
    ) -> Result<Vec<TeamPendingGrantView>, TeamError> {
        self.team_ws(ws).await?;
        let grants = self
            .deps
            .workspaces
            .pending_grants(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_DATASTORE_UNAVAILABLE"))?;
        let mut out: Vec<TeamPendingGrantView> = grants
            .into_iter()
            .filter(|g| g.state == ordin_workspaces::PendingGrantState::Waiting)
            .map(|g| TeamPendingGrantView {
                grant_id: g.id,
                added_by: g.grant.added_by.clone(),
                agent_name: g.grant.agent_name.clone(),
                new_agent: g.grant.agent_instance_id.is_none(),
                access: g
                    .grant
                    .access
                    .iter()
                    .filter(|a| a.place_id.is_some())
                    .map(|a| TeamAccessLine {
                        place_name: a.place_name.clone(),
                        access: a.access,
                    })
                    .collect(),
                left_out: g
                    .grant
                    .access
                    .iter()
                    .filter(|a| a.place_id.is_none())
                    .map(|a| a.place_name.clone())
                    .collect(),
                added_at: g.grant.added_at,
                added_by_confirmed: g.grant.added_by_confirmed,
            })
            .collect();
        out.sort_by_key(|g| std::cmp::Reverse(g.added_at));
        Ok(out)
    }

    async fn waiting_grant(
        &self,
        ws: WorkspaceId,
        grant: TeamGrantId,
    ) -> Result<ordin_workspaces::PendingGrant, TeamError> {
        self.team_ws(ws).await?;
        let grants = self
            .deps
            .workspaces
            .pending_grants(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_DATASTORE_UNAVAILABLE"))?;
        grants
            .into_iter()
            .find(|g| g.id == grant && g.state == ordin_workspaces::PendingGrantState::Waiting)
            .ok_or_else(|| TeamError::permanent("TEAM_WS_GRANT_NOT_FOUND"))
    }

    pub async fn allow_grant(
        &self,
        ws: WorkspaceId,
        grant: TeamGrantId,
    ) -> Result<TeamKnowledgeStatus, TeamError> {
        let guard = self.workspace_lock(ws);
        let held = guard.lock().await;
        let g = self.waiting_grant(ws, grant).await?;
        let agent = match g.grant.agent_instance_id {
            Some(a) => a,
            None => self
                .deps
                .agents
                .add_agent(ws, &g.grant.repository, &g.grant.agent_id, &g.grant.agent_version)
                .await
                .map_err(|e| {
                    tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.agent_not_added");
                    TeamError::permanent("TEAM_WS_AGENT_NOT_ADDED")
                })?,
        };
        self.put_agent_row(ws, &g.grant.agent_key, Some(agent), "present", None)
            .await?;
        let mut entries = 0u32;
        for a in &g.grant.access {
            // Places this person cannot open are left out; a place linked since the grant was asked
            // is looked up again.
            let place = match a.place_id {
                Some(p) => Some(p),
                None => self.place_id_of(ws, &a.place_key).await?,
            };
            let Some(place) = place else {
                continue;
            };
            self.deps
                .workspaces
                .set_grant(
                    ws,
                    Grantee::AgentInstance {
                        agent_instance_id: agent,
                    },
                    place,
                    Some(a.access),
                )
                .await
                .map_err(|e| ws_error(&e, "TEAM_WS_ALLOW_FAILED"))?;
            entries += 1;
        }
        self.deps
            .workspaces
            .decide_pending_grant(grant, GrantDecision::Allowed)
            .await
            .map_err(|e| match e {
                e if e.code() == "WS_GRANT_NOT_FOUND" => {
                    TeamError::permanent("TEAM_WS_GRANT_NOT_FOUND")
                }
                e => ws_error(&e, "TEAM_WS_ALLOW_FAILED"),
            })?;
        self.journal(
            "grant_allowed",
            ws,
            "A change your team made was allowed.",
            serde_json::json!({ "workspace_id": ws, "grant_id": grant, "entries": entries }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), "team.grant_allowed");
        self.changed(ws).await;
        drop(held);
        self.status(ws).await
    }

    pub async fn decline_grant(
        &self,
        ws: WorkspaceId,
        grant: TeamGrantId,
    ) -> Result<TeamKnowledgeStatus, TeamError> {
        let guard = self.workspace_lock(ws);
        let held = guard.lock().await;
        let g = self.waiting_grant(ws, grant).await?;
        self.deps
            .workspaces
            .decide_pending_grant(grant, GrantDecision::Declined)
            .await
            .map_err(|e| match e {
                e if e.code() == "WS_GRANT_NOT_FOUND" => {
                    TeamError::permanent("TEAM_WS_GRANT_NOT_FOUND")
                }
                e => ws_error(&e, "TEAM_WS_ALLOW_FAILED"),
            })?;
        let state = if g.grant.agent_instance_id.is_some() {
            "present"
        } else {
            "declined"
        };
        sqlx::query(
            "INSERT INTO team_workspace_agents (workspace_id, key, agent_instance_id, state, declined_changed_at) VALUES (?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT (workspace_id, key) DO UPDATE SET state = excluded.state, declined_changed_at = excluded.declined_changed_at",
        )
        .bind(ws.to_string())
        .bind(&g.grant.agent_key)
        .bind(g.grant.agent_instance_id.map(|a| a.to_string()))
        .bind(state)
        .bind(g.grant.added_at.to_wire())
        .execute(self.pool()?)
        .await?;
        self.journal(
            "grant_declined",
            ws,
            "A change your team made was not allowed.",
            serde_json::json!({ "workspace_id": ws, "grant_id": grant }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), "team.grant_declined");
        self.changed(ws).await;
        drop(held);
        self.status(ws).await
    }

    // ---- TW-F ----

    /// TW-F step 1: one entry per ended task run of a team workspace, then TW-G step 2.
    pub(crate) async fn record_run(&self, run_id: RunId) {
        let mut facts = self.deps.runs.ended_task(run_id).await;
        if matches!(&facts, Err(e) if e.retryable) {
            self.wait(std::time::Duration::from_secs(5)).await;
            facts = self.deps.runs.ended_task(run_id).await;
        }
        let facts = match facts {
            Ok(Some(f)) => f,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!(target: "ordin::team_knowledge", run_id = run_id.to_string().as_str(), code = e.code, "team.activity_failed");
                return;
            }
        };
        let ws = facts.workspace_id;
        let Ok(Some(row)) = self.team_row(ws).await else {
            return;
        };
        if row.state != "team" {
            return;
        }
        let outcome = match facts.state {
            ordin_types::domain::RunState::Completed => "completed",
            ordin_types::domain::RunState::Failed
            | ordin_types::domain::RunState::NoReply
            | ordin_types::domain::RunState::Abandoned => "failed",
            ordin_types::domain::RunState::StoppedAtLimit
            | ordin_types::domain::RunState::Cancelled => "stopped",
            _ => return,
        };
        let mut results = self
            .deps
            .workspaces
            .results_of_run(ws, run_id)
            .await
            .unwrap_or_default();
        results.sort();
        results.truncate(50);
        let Ok(pool) = self.pool().cloned() else {
            return;
        };
        let inserted = sqlx::query(
            "INSERT INTO team_activity (id, workspace_id, mine, kind, by_login, by_confirmed, at, task, agent, outcome, results, change, subject, replaced_by, replaced_by_confirmed, run_id, blob, shared_at) \
             VALUES (?1, ?2, 1, 'run', NULL, 1, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, 0, ?8, NULL, NULL) ON CONFLICT DO NOTHING",
        )
        .bind(TeamActivityId::new().to_string())
        .bind(ws.to_string())
        .bind(facts.ended_at.to_wire())
        .bind(facts.task_title.as_ref().map(|t| t.chars().take(200).collect::<String>()))
        .bind(facts.agent_name.chars().take(120).collect::<String>())
        .bind(outcome)
        .bind(store::json(&results))
        .bind(run_id.to_string())
        .execute(&pool)
        .await;
        match inserted {
            Ok(r) if r.rows_affected() == 1 => {}
            _ => return,
        }
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), results = results.len(), "team.activity_recorded");
        self.changed(ws).await;
        // TW-G step 2.
        if results.is_empty() && row.results_kind != "team_folder" {
            return;
        }
        let guard = self.workspace_lock(ws);
        let _held = guard.lock().await;
        let Ok(b) = self.bound(ws).await else {
            return;
        };
        if b.access == TeamAccess::ReadOnly {
            return;
        }
        if row.results_kind == "team_folder" {
            if matches!(store::open_share(&pool, ws).await, Ok(Some(_))) {
                return;
            }
            if let Err(e) = self.share_now(ws, ShareScope::Results).await {
                tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.results_share_failed");
                let _ = store::set_last_error(&pool, ws, Some(e.code)).await;
            }
            self.changed(ws).await;
        } else {
            if let Err(e) = self.publish_results(ws, &row, &results).await {
                tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.results_publish_failed");
            }
            self.changed(ws).await;
        }
    }

    /// TW-G step 3 (amended January 2026, the user's decision): each unsent result of the run goes to
    /// the SharePoint folder or Confluence page of the results link as a published copy, one card each.
    // spec: ordin-desktop/components/TeamKnowledgeService#TW-G
    async fn publish_results(
        &self,
        ws: WorkspaceId,
        row: &TeamRow,
        results: &[String],
    ) -> Result<(), TeamError> {
        const AT: &str = "ordin-team-workspace.json#results";
        let pool = self.pool()?.clone();
        let Some(folder) = self.deps.workspaces.results_folder(ws).await.ok().flatten() else {
            return Ok(());
        };
        let unsent: Vec<_> = self
            .deps
            .workspaces
            .unsent_results(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_DATASTORE_UNAVAILABLE"))?
            .into_iter()
            .filter(|r| results.contains(&r.path))
            .collect();
        if unsent.is_empty() {
            return Ok(());
        }
        // Where it goes: the results place's key → its link → the location it names.
        let link_id = self
            .place_rows(ws)
            .await?
            .into_iter()
            .find(|(k, _, _)| Some(k) == row.results_place_key.as_ref())
            .and_then(|(_, link, _)| link);
        let links = self
            .deps
            .workspaces
            .source_links(ws)
            .await
            .unwrap_or_default();
        let link = link_id.and_then(|id| {
            links
                .into_iter()
                .find(|l| l.id == id && l.state == SourceLinkState::Active)
        });
        let Some(link) = link else {
            store::put_problem(&pool, ws, AT, "TEAM_WS_RESULTS_NO_ACCESS", "").await?;
            return Ok(());
        };
        let location = match self
            .deps
            .workspaces
            .resolve_origin(link.kind, &link.origin_location)
            .await
        {
            Ok(location) => location,
            Err(e) => {
                tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code(), "team.results_place_unread");
                store::put_problem(&pool, ws, AT, "TEAM_WS_RESULTS_NO_ACCESS", "").await?;
                return Ok(());
            }
        };
        let Some(destination) = results_destination(location) else {
            store::put_problem(&pool, ws, AT, "TEAM_WS_RESULTS_NOT_SUITABLE", "").await?;
            return Ok(());
        };
        store::clear_problem(&pool, ws, AT).await?;
        let mut sent = Vec::new();
        for r in unsent {
            let name = r.path.rsplit('/').next().unwrap_or(&r.path).to_owned();
            let request = ordin_push::PublishCopyRequest {
                workspace_id: ws,
                original: CopyOriginal::PlaceFile {
                    place_id: folder,
                    path: r.path.clone(),
                },
                original_version: r.version.to_string(),
                original_name: name.clone(),
                page_file: None,
                destination: destination.clone(),
                format: CopyFormat::ExactFile,
                followed_branch: None,
                keep_up_to_date: false,
                file_name: name,
            };
            let at = format!("results/{}", r.path);
            match self.deps.push.publish_result(request).await {
                Ok(_) => {
                    store::clear_problem(&pool, ws, &at).await?;
                    sent.push((r.path, r.sha256));
                }
                Err(e) => {
                    tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code, "team.result_not_published");
                    store::put_problem(&pool, ws, &at, e.code, "").await?;
                }
            }
        }
        if !sent.is_empty() {
            let count = sent.len();
            self.deps
                .workspaces
                .mark_results_sent(ws, sent)
                .await
                .map_err(|e| ws_error(&e, "TEAM_DATASTORE_UNAVAILABLE"))?;
            tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), results = count, "team.results_shared");
        }
        Ok(())
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-F
    pub async fn activity(
        &self,
        ws: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<Page<TeamActivityView>, TeamError> {
        let row = self.team_ws(ws).await?;
        let pool = self.pool()?;
        let after: Option<(String, String)> = match &cursor {
            Some(c) => Some(
                c.decode()
                    .map_err(|_| TeamError::permanent("TEAM_WS_NOT_TEAM"))?,
            ),
            None => None,
        };
        let rows = match &after {
            None => sqlx::query("SELECT * FROM team_activity WHERE workspace_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2")
                .bind(ws.to_string())
                .bind((ACTIVITY_PAGE + 1) as i64)
                .fetch_all(pool)
                .await?,
            Some((at, id)) => sqlx::query("SELECT * FROM team_activity WHERE workspace_id = ?1 AND (at < ?2 OR (at = ?2 AND id < ?3)) ORDER BY at DESC, id DESC LIMIT ?4")
                .bind(ws.to_string())
                .bind(at)
                .bind(id)
                .bind((ACTIVITY_PAGE + 1) as i64)
                .fetch_all(pool)
                .await?,
        };
        let results_place = if row.results_kind == "team_folder" {
            self.deps.workspaces.results_folder(ws).await.ok().flatten()
        } else {
            None
        };
        let records = self.file_records(ws).await?;
        let more = rows.len() > ACTIVITY_PAGE;
        let mut items = Vec::new();
        let mut last = None;
        for r in rows.iter().take(ACTIVITY_PAGE) {
            let id: String = r.try_get("id")?;
            let at: String = r.try_get("at")?;
            let mine = r.try_get::<i64, _>("mine")? == 1;
            let kind: String = r.try_get("kind")?;
            let list: String = r.try_get("results")?;
            let paths: Vec<String> = serde_json::from_str(&list).unwrap_or_default();
            let outcome: Option<String> = r.try_get("outcome")?;
            let replaced_by: Option<String> = r.try_get("replaced_by")?;
            let results = paths
                .iter()
                .map(|p| TeamResultRef {
                    name: p.rsplit('/').next().unwrap_or(p).to_owned(),
                    file: results_place
                        .filter(|_| mine || records.contains_key(&format!("results/{p}")))
                        .map(|place_id| ContentRef::PlaceFile {
                            place_id,
                            relative_path: p.clone(),
                        }),
                })
                .collect();
            items.push(TeamActivityView {
                id: TeamActivityId::parse(&id).map_err(|_| TeamError::datastore())?,
                kind: if kind == "replaced" {
                    TeamActivityKind::Replaced
                } else {
                    TeamActivityKind::Run
                },
                by: if mine && kind == "run" {
                    r.try_get::<Option<String>, _>("blob")?
                        .and(r.try_get("by_login")?)
                } else {
                    r.try_get("by_login")?
                },
                by_confirmed: r.try_get::<i64, _>("by_confirmed")? == 1,
                mine,
                at: store::ts(Some(at.clone())).unwrap_or_else(Timestamp::now),
                task: r.try_get("task")?,
                agent: r.try_get("agent")?,
                outcome: outcome.as_deref().map(|o| match o {
                    "completed" => TeamRunOutcome::Completed,
                    "failed" => TeamRunOutcome::Failed,
                    _ => TeamRunOutcome::Stopped,
                }),
                results,
                change: r.try_get("change")?,
                subject: r.try_get("subject")?,
                replaced_by_confirmed: replaced_by.is_some()
                    && r.try_get::<i64, _>("replaced_by_confirmed")? == 1,
                replaced_by,
            });
            last = Some((at, id));
        }
        Ok(Page {
            items,
            next_cursor: if more {
                last.map(|k| Cursor::encode(&k))
            } else {
                None
            },
        })
    }

    // ---- results_choices and set_results_place, TW-K (amended January 2026, the user's decision) ----

    /// TW-K step 1: the choices of **Change…** after sharing, read from this computer only.
    // spec: ordin-desktop/components/TeamKnowledgeService#TW-K
    pub async fn results_choices(&self, ws: WorkspaceId) -> Result<TeamResultsChoices, TeamError> {
        let row = self.team_ws(ws).await?;
        let Some(b) = store::binding(self.pool()?, ws).await? else {
            return Err(TeamError::permanent("TEAM_WS_NOT_TEAM"));
        };
        let links = self
            .deps
            .workspaces
            .source_links(ws)
            .await
            .map_err(|e| ws_error(&e, "TEAM_WORKSPACE_NOT_FOUND"))?;
        let choices = results_options(format!("{} › results", b.header_line()), &links);
        let current = if row.results_kind == "team_folder" {
            Some(TeamResultsChoice::TeamFolder {})
        } else {
            self.place_rows(ws)
                .await?
                .into_iter()
                .find(|(k, _, _)| Some(k) == row.results_place_key.as_ref())
                .and_then(|(_, link, _)| link)
                .map(|source_link_id| TeamResultsChoice::Link { source_link_id })
        };
        Ok(TeamResultsChoices { choices, current })
    }

    /// TW-K step 2.
    // spec: ordin-desktop/components/TeamKnowledgeService#TW-K
    pub async fn set_results_place(
        &self,
        ws: WorkspaceId,
        results: TeamResultsChoice,
    ) -> Result<TeamKnowledgeStatus, TeamError> {
        let guard = self.workspace_lock(ws);
        let held = guard.lock().await;
        self.team_ws(ws).await?;
        match self.check_results_choice(ws, &results).await? {
            None => self.set_results(ws, "team_folder", None).await?,
            Some(link) => {
                let rows = self.place_rows(ws).await?;
                let key = match rows.iter().find(|(_, l, _)| *l == Some(link.id)) {
                    Some((k, _, _)) => k.clone(),
                    None => {
                        let k = uuid7();
                        self.put_place_row(ws, &k, Some(link.id), "linked").await?;
                        k
                    }
                };
                let kind = if link.kind == SourceLinkKind::SharepointLocation {
                    "sharepoint"
                } else {
                    "confluence"
                };
                self.set_results(ws, kind, Some(&key)).await?;
            }
        }
        self.changed(ws).await;
        drop(held);
        self.status(ws).await
    }

    // spec: ordin-desktop/components/TeamKnowledgeService#TW-H
    pub async fn leave_workspace(&self, ws: WorkspaceId) -> Result<(), TeamError> {
        let guard = self.workspace_lock(ws);
        let _held = guard.lock().await;
        self.leave_locked(ws).await
    }

    pub(crate) async fn leave_locked(&self, ws: WorkspaceId) -> Result<(), TeamError> {
        let pool = self.pool()?.clone();
        if self.team_row(ws).await?.is_none() {
            return Err(TeamError::permanent("TEAM_WS_NOT_TEAM"));
        }
        if let Some(open) = store::open_share(&pool, ws).await? {
            match open.state.as_str() {
                "approved" | "sending" => return Err(TeamError::permanent("TEAM_SHARE_PENDING")),
                "proposed" => {
                    let _ = self.deps.push.withdraw_team_share(open.id).await;
                    store::set_share_state(&pool, open.id, "abandoned", None).await?;
                }
                _ => {}
            }
        }
        self.delete_team_rows(ws).await?;
        if let Err(e) = self.deps.workspaces.end_team(ws).await {
            tracing::warn!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), code = e.code(), "team.end_team_failed");
        }
        if store::binding(&pool, ws).await?.is_some() {
            self.turn_off_locked(ws, "left").await?;
        }
        self.journal(
            "workspace_left",
            ws,
            "This person left the team workspace.",
            serde_json::json!({ "workspace_id": ws }),
        )
        .await;
        tracing::info!(target: "ordin::team_knowledge", workspace_id = ws.to_string().as_str(), "team.workspace_left");
        self.changed(ws).await;
        Ok(())
    }

    // ---- status ----

    pub(crate) async fn team_workspace_status(
        &self,
        ws: WorkspaceId,
        b: Option<&Binding>,
    ) -> Result<Option<TeamWorkspaceStatus>, TeamError> {
        let Some(row) = self.team_row(ws).await? else {
            return Ok(None);
        };
        let links = self
            .deps
            .workspaces
            .source_links(ws)
            .await
            .unwrap_or_default();
        let synced = row.synced.clone();
        let place_name = |key: &str, link: Option<SourceLinkId>| -> String {
            link.and_then(|l| {
                links
                    .iter()
                    .find(|x| x.id == l)
                    .map(|x| x.display_name.clone())
            })
            .or_else(|| {
                synced.as_ref().and_then(|s| {
                    s.places
                        .iter()
                        .find(|p| p.key == key)
                        .map(|p| p.display_name.clone())
                })
            })
            .unwrap_or_default()
        };
        let mut places = Vec::new();
        let place_rows = self.place_rows(ws).await?;
        for (key, link, state) in &place_rows {
            let state = match state.as_str() {
                "no_access" => TeamPlaceState::NoAccess,
                "waiting" => TeamPlaceState::Waiting,
                "removal_waits" => TeamPlaceState::RemovalWaits,
                _ => TeamPlaceState::Linked,
            };
            places.push(TeamPlaceStatus {
                key: key.clone(),
                display_name: place_name(key, *link),
                state,
            });
        }
        let mut agents_not_here = Vec::new();
        for (key, _, state, _) in self.agent_rows(ws).await? {
            if state == "declined" || state == "not_found" {
                if let Some(a) = synced
                    .as_ref()
                    .and_then(|s| s.agents.iter().find(|a| a.key == key))
                {
                    agents_not_here.push(TeamAgentLine {
                        key: key.clone(),
                        name: a.name.clone(),
                        access: access_lines(synced.as_ref().expect("synced"), a),
                        found: None,
                    });
                }
            }
        }
        let header = b.map(|b| b.header_line()).unwrap_or_default();
        let results = match &row.results_place_key {
            None => format!("{header} › results"),
            Some(key) => {
                let link = place_rows
                    .iter()
                    .find(|(k, _, _)| k == key)
                    .and_then(|(_, l, _)| *l);
                place_name(key, link)
            }
        };
        let blocked = row.results_place_key.as_ref().is_some_and(|k| {
            place_rows
                .iter()
                .any(|(key, _, st)| key == k && st == "no_access")
        });
        let pending = self
            .deps
            .workspaces
            .pending_grants(ws)
            .await
            .map(|g| {
                g.into_iter()
                    .filter(|g| g.state == ordin_workspaces::PendingGrantState::Waiting)
                    .count()
            })
            .unwrap_or(0);
        let unsent = self
            .deps
            .workspaces
            .unsent_results(ws)
            .await
            .map(|r| r.len())
            .unwrap_or(0);
        let waiting: i64 = sqlx::query_scalar("SELECT count(*) FROM team_activity WHERE workspace_id = ?1 AND mine = 1 AND blob IS NULL")
            .bind(ws.to_string())
            .fetch_one(self.pool()?)
            .await?;
        Ok(Some(TeamWorkspaceStatus {
            state: if row.state == "team" {
                TeamWorkspaceState::Team
            } else {
                TeamWorkspaceState::Proposed
            },
            places,
            agents_not_here,
            results,
            results_here_only: b.is_some_and(|b| b.access == TeamAccess::ReadOnly) || blocked,
            pending_grants: pending as u32,
            unsent_results: unsent as u32,
            activity_waiting: waiting as u32,
        }))
    }
}

use crate::ports::TeamAgentFind;

fn lock_set_busy(service: &TeamKnowledgeService, ws: WorkspaceId) {
    service.set_busy(ws, TeamBusy::Opening);
}

#[derive(Debug, Clone)]
enum NameCheck {
    Setting(Setting, String, Timestamp),
}

/// The names a description gives, for TW-J at an open.
fn description_checks(
    file: &TeamWorkspaceFile,
    _synced: Option<&TeamWorkspaceFile>,
) -> Vec<NameCheck> {
    settings_of(file)
        .into_iter()
        .map(|(s, by, at)| NameCheck::Setting(s, by, at))
        .collect()
}

fn settings_of(f: &TeamWorkspaceFile) -> Vec<(Setting, String, Timestamp)> {
    let mut out = vec![(Setting::Name, f.name_changed_by.clone(), f.name_changed_at)];
    out.extend(f.places.iter().map(|p| {
        (
            Setting::Place(p.key.clone()),
            p.changed_by.clone(),
            p.changed_at,
        )
    }));
    out.extend(f.agents.iter().map(|a| {
        (
            Setting::Agent(a.key.clone()),
            a.changed_by.clone(),
            a.changed_at,
        )
    }));
    out.push((
        Setting::Results,
        f.results.changed_by.clone(),
        f.results.changed_at,
    ));
    out
}

/// True when `f` holds `setting` with the stamps `by` and `at`.
fn setting_holds(f: &TeamWorkspaceFile, s: &Setting, by: &str, at: Timestamp) -> bool {
    settings_of(f)
        .into_iter()
        .any(|(x, b, a)| &x == s && b == by && a == at)
}

/// A setting's value without its stamps, as JSON; None when absent.
fn setting_value(f: &TeamWorkspaceFile, s: &Setting) -> Option<serde_json::Value> {
    match s {
        Setting::Name => Some(serde_json::json!(f.name)),
        Setting::Place(k) => f
            .places
            .iter()
            .find(|p| &p.key == k)
            .map(|p| serde_json::json!([p.kind, p.location, p.display_name])),
        Setting::Agent(k) => f.agents.iter().find(|a| &a.key == k).map(agent_value),
        Setting::Results => Some(serde_json::json!([f.results.kind, f.results.place])),
    }
}

/// A setting with its stamps.
fn setting_full(f: &TeamWorkspaceFile, s: &Setting) -> Option<serde_json::Value> {
    settings_of(f)
        .into_iter()
        .find(|(x, _, _)| x == s)
        .map(|(_, by, at)| serde_json::json!([setting_value(f, s), by, at]))
}

fn agent_value(a: &TeamAgentEntry) -> serde_json::Value {
    serde_json::json!([a.repository, a.id, a.version, a.name, a.access])
}

/// Copies `s` from `from` into `into` (removing it when `from` lacks it).
fn set_setting(into: &mut TeamWorkspaceFile, from: &TeamWorkspaceFile, s: &Setting) {
    match s {
        Setting::Name => {
            into.name = from.name.clone();
            into.name_changed_by = from.name_changed_by.clone();
            into.name_changed_at = from.name_changed_at;
        }
        Setting::Place(k) => {
            into.places.retain(|p| &p.key != k);
            into.places
                .extend(from.places.iter().filter(|p| &p.key == k).cloned());
            into.places.sort_by(|a, b| a.key.cmp(&b.key));
        }
        Setting::Agent(k) => {
            into.agents.retain(|a| &a.key != k);
            into.agents
                .extend(from.agents.iter().filter(|a| &a.key == k).cloned());
            into.agents.sort_by(|a, b| a.key.cmp(&b.key));
        }
        Setting::Results => into.results = from.results.clone(),
    }
}

/// Who changed a setting in R, and the name of what changed.
fn setting_who(r: &TeamWorkspaceFile, s_file: &TeamWorkspaceFile, s: &Setting) -> (String, String) {
    let pick = |f: &TeamWorkspaceFile| -> Option<(String, String)> {
        match s {
            Setting::Name => Some((f.name_changed_by.clone(), f.name.clone())),
            Setting::Place(k) => f
                .places
                .iter()
                .find(|p| &p.key == k)
                .map(|p| (p.changed_by.clone(), p.display_name.clone())),
            Setting::Agent(k) => f
                .agents
                .iter()
                .find(|a| &a.key == k)
                .map(|a| (a.changed_by.clone(), a.name.clone())),
            Setting::Results => Some((f.results.changed_by.clone(), "results".to_owned())),
        }
    };
    pick(r).or_else(|| pick(s_file)).unwrap_or_default()
}

/// One of TEAM_CHANGES for a replaced setting.
fn change_name(
    s: &Setting,
    synced: &TeamWorkspaceFile,
    remote: &TeamWorkspaceFile,
) -> &'static str {
    match s {
        Setting::Name => "renamed",
        Setting::Results => "results_place_changed",
        Setting::Place(k) => match (
            synced.places.iter().any(|p| &p.key == k),
            remote.places.iter().any(|p| &p.key == k),
        ) {
            (false, true) => "place_added",
            (true, false) => "place_removed",
            _ => "place_added",
        },
        Setting::Agent(k) => match (
            synced.agents.iter().any(|a| &a.key == k),
            remote.agents.iter().any(|a| &a.key == k),
        ) {
            (false, true) => "agent_added",
            (true, false) => "agent_removed",
            _ => "access_changed",
        },
    }
}

/// TW-B step 3: the subjects of the settings that differ from the synced description.
fn changed_subjects(synced: Option<&TeamWorkspaceFile>, now: &TeamWorkspaceFile) -> Vec<String> {
    let Some(s) = synced else {
        return Vec::new();
    };
    let mut ids: BTreeSet<Setting> = settings_of(s).into_iter().map(|(x, _, _)| x).collect();
    ids.extend(settings_of(now).into_iter().map(|(x, _, _)| x));
    ids.into_iter()
        .filter(|id| setting_value(s, id) != setting_value(now, id))
        .map(|id| setting_who(now, s, &id).1)
        .collect()
}

fn access_lines(file: &TeamWorkspaceFile, a: &TeamAgentEntry) -> Vec<TeamAccessLine> {
    a.access
        .iter()
        .map(|e| TeamAccessLine {
            place_name: match e.place.as_str() {
                "own" => file.name.clone(),
                "results" => ordin_workspaces::TEAM_RESULTS_NAME.to_owned(),
                key => file
                    .places
                    .iter()
                    .find(|p| p.key == key)
                    .map(|p| p.display_name.clone())
                    .unwrap_or_default(),
            },
            access: access_of(&e.access),
        })
        .collect()
}

fn results_name(file: &TeamWorkspaceFile, header: &str) -> String {
    match &file.results.place {
        None => format!("{header} › results"),
        Some(k) => file
            .places
            .iter()
            .find(|p| &p.key == k)
            .map(|p| p.display_name.clone())
            .unwrap_or_default(),
    }
}

/// An activity row as its file, `by` the login (TW-F step 3).
fn activity_file_of(
    r: &sqlx::sqlite::SqliteRow,
    login: Option<&str>,
) -> Result<TeamActivityFile, TeamError> {
    let id: String = r.try_get("id")?;
    let list: String = r.try_get("results")?;
    let kind: String = r.try_get("kind")?;
    let by: Option<String> = r.try_get("by_login")?;
    let replaced_by: Option<String> = r.try_get("replaced_by")?;
    Ok(TeamActivityFile {
        format: 1,
        id: TeamActivityId::parse(&id).map_err(|_| TeamError::datastore())?,
        by: if kind == "run" {
            login.map(str::to_owned).or(by).unwrap_or_default()
        } else {
            by.unwrap_or_default()
        },
        kind,
        at: store::ts(r.try_get("at")?).unwrap_or_else(Timestamp::now),
        task: r.try_get("task")?,
        agent: r.try_get("agent")?,
        outcome: r.try_get("outcome")?,
        results: serde_json::from_str(&list).unwrap_or_default(),
        change: r.try_get("change")?,
        subject: r.try_get("subject")?,
        replaced_by,
    })
}
