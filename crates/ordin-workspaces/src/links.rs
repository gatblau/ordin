//! Places and source links: proposing, confirming, declining and removing a link, and the mapping of
//! adapter errors (WorkspaceService.md Internal Logic 3 and 5). The reading of a remote link into its
//! working copy is `remote.rs`; the Jira reading is `jira.rs`.

use std::path::PathBuf;

use ordin_files::{AddPlace, FileError, Place, PlaceKind};
use ordin_types::domain::places::{SourceLink, SourceLinkKind, SourceLinkState};
use ordin_types::time::Timestamp;
use ordin_types::{CodedError, PlaceId, SourceLinkId, WorkspaceId};
use sqlx::Row;

use crate::error::{file_failed, is_unique_violation, WorkspaceError};
use crate::log;
use crate::service::{file_name, lock, WorkspaceService};
use crate::store::{self, to_text};
use crate::types::{LinkLocation, LinkProgress};

/// What a location resolves to before any row or place exists.
struct Prepared {
    kind: SourceLinkKind,
    origin_location: String,
    default_name: String,
    /// Local and git folders: the canonical folder that becomes a place.
    local_path: Option<PathBuf>,
}

/// The adapter-independent meaning of an adapter error code (Internal Logic 3.5 and §J.6).
pub(crate) fn map_adapter_code(code: &str) -> WorkspaceError {
    let tail = code.split_once('_').map_or(code, |(_, tail)| tail);
    match tail {
        "SIGN_IN_NEEDED" | "NOT_CONNECTED" | "TOKEN_REFUSED" | "SITE_NOT_GRANTED" => {
            WorkspaceError::SourceSignInNeeded
        }
        "NOT_CONFIGURED" => WorkspaceError::SourceNotConfigured,
        "ACCESS_DENIED" => WorkspaceError::SourceAccessDenied,
        "NOT_FOUND" => WorkspaceError::SourceNotFound,
        "TOO_LARGE" => WorkspaceError::ItemTooLarge,
        "SITE_MISMATCH" => WorkspaceError::JiraSiteMismatch,
        "LOCATION_INVALID" => WorkspaceError::Invalid("SRC_LOCATION_INVALID"),
        // UNAVAILABLE, THROTTLED, RESPONSE_INVALID and every other code the adapters raise.
        _ => WorkspaceError::SourceUnavailable,
    }
}

pub(crate) fn map_adapter<E: CodedError>(error: &E) -> WorkspaceError {
    map_adapter_code(error.code())
}

pub(crate) fn kind_name(kind: SourceLinkKind) -> &'static str {
    match kind {
        SourceLinkKind::LocalFolder => "local_folder",
        SourceLinkKind::GitRepositoryFolder => "git_repository_folder",
        SourceLinkKind::SharepointLocation => "sharepoint_location",
        SourceLinkKind::ConfluenceLocation => "confluence_location",
        SourceLinkKind::GitRemote => "git_remote",
        SourceLinkKind::JiraIssue => "jira_issue",
    }
}

/// `SRC_NAME_INVALID`: 1–120 characters after trimming.
pub(crate) fn check_display_name(name: &str) -> Result<String, WorkspaceError> {
    let trimmed = name.trim();
    let chars = trimmed.chars().count();
    if chars == 0 || chars > 120 {
        return Err(WorkspaceError::Invalid("SRC_NAME_INVALID"));
    }
    Ok(trimmed.to_owned())
}

/// `SRC_LOCATION_INVALID`: 1–2,048 bytes.
pub(crate) fn check_origin(origin_location: &str) -> Result<(), WorkspaceError> {
    if origin_location.is_empty() || origin_location.len() > 2048 {
        return Err(WorkspaceError::Invalid("SRC_LOCATION_INVALID"));
    }
    Ok(())
}

/// `SRC_PLACE_MISMATCH`: a local or git folder link names its place; a link with no place of its own
/// (`git_remote`) names none; the remote kinds may name their working copy.
pub(crate) fn check_place_rule(
    kind: SourceLinkKind,
    place_id: Option<PlaceId>,
) -> Result<(), WorkspaceError> {
    let ok = match kind {
        SourceLinkKind::LocalFolder | SourceLinkKind::GitRepositoryFolder => place_id.is_some(),
        SourceLinkKind::GitRemote => place_id.is_none(),
        SourceLinkKind::SharepointLocation
        | SourceLinkKind::ConfluenceLocation
        | SourceLinkKind::JiraIssue => true,
    };
    if ok {
        Ok(())
    } else {
        Err(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))
    }
}

pub(crate) fn cut(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// A parsed `GitRemote` location: the origin form and the default display name (SA-CLOSE-03).
pub(crate) fn parse_git_remote(
    url: &str,
    branch: &str,
    path: &str,
) -> Result<(String, String), WorkspaceError> {
    let invalid = || WorkspaceError::GitRemoteInvalid;
    let name_ok = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    let host_ok = |host: &str| {
        !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    };
    let (owner, name) = if let Some(rest) = url.strip_prefix("git@") {
        let (host, repo) = rest.split_once(':').ok_or_else(invalid)?;
        if !host_ok(host) {
            return Err(invalid());
        }
        let (owner, name) = repo.split_once('/').ok_or_else(invalid)?;
        (owner.to_owned(), name.to_owned())
    } else {
        let parsed = url::Url::parse(url).map_err(|_| invalid())?;
        if parsed.scheme() != "https"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.host_str().is_some_and(host_ok)
        {
            return Err(invalid());
        }
        let segments: Vec<&str> = parsed.path_segments().ok_or_else(invalid)?.collect();
        if segments.len() != 2 {
            return Err(invalid());
        }
        (segments[0].to_owned(), segments[1].to_owned())
    };
    let bare = name.strip_suffix(".git").unwrap_or(&name).to_owned();
    if !name_ok(&owner) || !name_ok(&bare) {
        return Err(invalid());
    }
    let branch_chars = branch.chars().count();
    if branch_chars == 0 || branch_chars > 255 || branch.chars().any(char::is_control) {
        return Err(invalid());
    }
    if path.len() > 1024
        || path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|segment| segment == "..")
    {
        return Err(invalid());
    }
    let bare_url = url.strip_suffix(".git").unwrap_or(url);
    let origin = format!("git:{bare_url}#{branch}:{path}");
    let default = if path.is_empty() {
        bare
    } else {
        format!("{bare}/{path}")
    };
    Ok((origin, default))
}

impl WorkspaceService {
    // ---- reads

    pub async fn places(&self, id: WorkspaceId) -> Result<Vec<Place>, WorkspaceError> {
        self.workspace(id).await?;
        let ids = self.place_ids_of(id).await?;
        self.files.places(&ids, false).await.map_err(file_failed)
    }

    pub async fn source_links(&self, id: WorkspaceId) -> Result<Vec<SourceLink>, WorkspaceError> {
        self.workspace(id).await?;
        self.links_of(id).await
    }

    pub(crate) async fn links_of(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<SourceLink>, WorkspaceError> {
        let rows =
            sqlx::query("SELECT * FROM source_links WHERE workspace_id = ?1 ORDER BY added_at, id")
                .bind(id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(store::link_row).collect()
    }

    /// `WORKING_COPY_ROOT/<link>`: `<documents>/ordin/Linked copies/<link>`, outside ordin's data
    /// root, so an agent the person gives the link to can reach it (WS-WC-1).
    // spec: ordin-desktop/components/WorkspaceService#WS-WC-1
    pub fn working_copy_dir(&self, link: SourceLinkId) -> PathBuf {
        self.linked_copies_root().join(link.to_string())
    }

    pub(crate) fn linked_copies_root(&self) -> PathBuf {
        self.documents_dir
            .join("ordin")
            .join(crate::LINKED_COPIES_DIR)
    }

    /// Removes the link's copy where WS-WC-1 puts it and where a link made before it did.
    pub(crate) async fn remove_working_copy_dir(&self, link: SourceLinkId) {
        let dirs = [
            self.working_copy_dir(link),
            self.data_root.join("working-copies").join(link.to_string()),
        ];
        let _ = tokio::task::spawn_blocking(move || {
            for dir in dirs {
                let _ = std::fs::remove_dir_all(dir);
            }
        })
        .await;
    }

    async fn link_exists(
        &self,
        workspace: WorkspaceId,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<bool, WorkspaceError> {
        let row = sqlx::query(
            "SELECT count(*) AS n FROM source_links WHERE workspace_id = ?1 AND kind = ?2 AND origin_location = ?3",
        )
        .bind(workspace.to_string())
        .bind(to_text(&kind))
        .bind(origin_location)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("n")? > 0)
    }

    /// WS-ADD-1: the workspace's `proposed` link of the same kind and origin as `location`, if any.
    async fn proposed_same(
        &self,
        workspace: WorkspaceId,
        location: LinkLocation,
    ) -> Result<Option<SourceLinkId>, WorkspaceError> {
        if matches!(
            location,
            LinkLocation::LocalFolder { .. } | LinkLocation::GitRepositoryFolder { .. }
        ) {
            return Ok(None);
        }
        let prepared = self.prepare(location).await?;
        let row = sqlx::query(
            "SELECT * FROM source_links WHERE workspace_id = ?1 AND kind = ?2 AND origin_location = ?3 \
             AND state = 'proposed'",
        )
        .bind(workspace.to_string())
        .bind(to_text(&prepared.kind))
        .bind(&prepared.origin_location)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| store::link_row(&r).map(|l| l.id)).transpose()
    }

    // ---- 3. Adding a link

    /// Internal Logic 3.1.
    pub async fn propose_link(
        &self,
        id: WorkspaceId,
        location: LinkLocation,
        display_name: Option<String>,
    ) -> Result<SourceLink, WorkspaceError> {
        self.ensure_active(id).await?;
        let given = display_name.map(|n| check_display_name(&n)).transpose()?;
        let prepared = self.prepare(location).await?;
        let name = match given {
            Some(name) => name,
            None => check_display_name(&cut(&prepared.default_name, 120))?,
        };
        check_origin(&prepared.origin_location)?;
        if self
            .link_exists(id, prepared.kind, &prepared.origin_location)
            .await?
        {
            return Err(WorkspaceError::Invalid("SRC_DUPLICATE"));
        }
        let place_id = match &prepared.local_path {
            Some(path) => Some(self.place_for_folder(path.clone()).await?),
            None => None,
        };
        check_place_rule(prepared.kind, place_id)?;
        let link = SourceLink {
            id: SourceLinkId::new(),
            workspace_id: id,
            kind: prepared.kind,
            origin_location: prepared.origin_location,
            display_name: name,
            place_id,
            state: SourceLinkState::Proposed,
            added_at: Timestamp::now(),
            last_read_at: None,
        };
        sqlx::query(
            "INSERT INTO source_links (id, workspace_id, kind, origin_location, display_name, place_id, state, added_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'proposed', ?7)",
        )
        .bind(link.id.to_string())
        .bind(id.to_string())
        .bind(to_text(&link.kind))
        .bind(&link.origin_location)
        .bind(&link.display_name)
        .bind(place_id.map(|p| p.to_string()))
        .bind(link.added_at.to_wire())
        .execute(&self.pool)
        .await
        .map_err(|error| {
            if is_unique_violation(&error) {
                WorkspaceError::Invalid("SRC_DUPLICATE")
            } else {
                error.into()
            }
        })?;
        self.record(
            "source_link_proposed",
            Some(id),
            "A location was proposed for a workspace.",
            serde_json::json!({ "source_link_id": link.id, "kind": kind_name(link.kind) }),
        )
        .await;
        log::source_link_event("source_link_proposed", link.id, kind_name(link.kind));
        Ok(link)
    }

    /// The origin, kind and default name of a location, with no side effect.
    async fn prepare(&self, location: LinkLocation) -> Result<Prepared, WorkspaceError> {
        match location {
            LinkLocation::LocalFolder { path } => {
                self.prepare_folder(path, SourceLinkKind::LocalFolder).await
            }
            LinkLocation::GitRepositoryFolder { path } => {
                self.prepare_folder(path, SourceLinkKind::GitRepositoryFolder)
                    .await
            }
            LinkLocation::Sharepoint { location } => {
                let tail = location
                    .path
                    .rsplit('/')
                    .find(|segment| !segment.is_empty())
                    .map(str::to_owned);
                let default_name = tail.unwrap_or_else(|| {
                    if location.drive_name.is_empty() {
                        location.site_name.clone()
                    } else {
                        location.drive_name.clone()
                    }
                });
                Ok(Prepared {
                    kind: SourceLinkKind::SharepointLocation,
                    origin_location: ordin_sharepoint::SharePointAdapter::origin_location(
                        &location,
                    ),
                    default_name,
                    local_path: None,
                })
            }
            LinkLocation::Confluence { location } => {
                if self.confluence.is_none() {
                    return Err(WorkspaceError::ConfluenceUnavailable);
                }
                Ok(Prepared {
                    kind: SourceLinkKind::ConfluenceLocation,
                    origin_location: ordin_confluence::ConfluenceAdapter::origin_location(
                        &location,
                    ),
                    default_name: location.title.clone(),
                    local_path: None,
                })
            }
            LinkLocation::GitRemote { url, branch, path } => {
                let (origin_location, default_name) = parse_git_remote(&url, &branch, &path)?;
                Ok(Prepared {
                    kind: SourceLinkKind::GitRemote,
                    origin_location,
                    default_name,
                    local_path: None,
                })
            }
            LinkLocation::JiraIssue { address } => {
                if self.jira.is_none() {
                    return Err(WorkspaceError::JiraUnavailable);
                }
                let issue = ordin_jira::JiraAdapter::parse_issue_address(&address)
                    .map_err(|_| WorkspaceError::JiraAddressInvalid)?;
                Ok(Prepared {
                    kind: SourceLinkKind::JiraIssue,
                    origin_location: ordin_jira::JiraAdapter::issue_address(&issue),
                    default_name: issue.key,
                    local_path: None,
                })
            }
        }
    }

    async fn prepare_folder(
        &self,
        path: String,
        kind: SourceLinkKind,
    ) -> Result<Prepared, WorkspaceError> {
        if self.on_server {
            return Err(WorkspaceError::LocalFolderOnServer);
        }
        let given = PathBuf::from(path);
        // spec: ordin-desktop/components/WorkspaceService#WS-PATH-1
        let canonical = tokio::task::spawn_blocking(move || {
            std::fs::canonicalize(given).map(ordin_types::paths::plain_form)
        })
        .await
        .map_err(|_| WorkspaceError::FilesFailed)?
        .map_err(|_| WorkspaceError::PlaceRefused {
            cause: "unreadable",
        })?;
        if self.rules.is_protected_location(&canonical) {
            return Err(WorkspaceError::PlaceRefused { cause: "protected" });
        }
        if kind == SourceLinkKind::GitRepositoryFolder && !canonical.join(".git").exists() {
            return Err(WorkspaceError::NotARepository);
        }
        let origin_location = canonical.to_string_lossy().into_owned();
        Ok(Prepared {
            kind,
            default_name: file_name(&origin_location),
            origin_location,
            local_path: Some(canonical),
        })
    }

    /// `files.add_place(kind = persons_folder)`; an existing place is reused.
    async fn place_for_folder(&self, path: PathBuf) -> Result<PlaceId, WorkspaceError> {
        match self
            .files
            .add_place(AddPlace {
                path,
                friendly_name: None,
                kind: PlaceKind::PersonsFolder,
            })
            .await
        {
            Ok(place) => Ok(place.id),
            Err(FileError::PlaceExists { existing }) => Ok(existing),
            Err(FileError::ProtectedLocation) => {
                Err(WorkspaceError::PlaceRefused { cause: "protected" })
            }
            Err(FileError::FolderUnreadable) => Err(WorkspaceError::PlaceRefused {
                cause: "unreadable",
            }),
            Err(other) => Err(file_failed(other)),
        }
    }

    /// Internal Logic 3.2. On a failure the link stays `proposed` and nothing is half-added.
    pub async fn confirm_link(&self, link: SourceLinkId) -> Result<SourceLink, WorkspaceError> {
        let link = store::load_link(&self.pool, link).await?;
        if link.state != SourceLinkState::Proposed {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        self.ensure_active(link.workspace_id).await?;
        match link.kind {
            SourceLinkKind::LocalFolder | SourceLinkKind::GitRepositoryFolder => {
                let place = link
                    .place_id
                    .ok_or(WorkspaceError::Invalid("SRC_PLACE_MISMATCH"))?;
                self.add_membership(link.workspace_id, place).await?;
                if let Err(error) = self.files.watch(&[place]).await {
                    log::background_failed("watch", error.code());
                }
            }
            SourceLinkKind::GitRemote => {}
            SourceLinkKind::SharepointLocation
            | SourceLinkKind::ConfluenceLocation
            | SourceLinkKind::JiraIssue => self.confirm_remote(&link).await?,
        }
        sqlx::query("UPDATE source_links SET state = 'active' WHERE id = ?1")
            .bind(link.id.to_string())
            .execute(&self.pool)
            .await?;
        self.record(
            "source_link_added",
            Some(link.workspace_id),
            "A location was added to a workspace.",
            serde_json::json!({ "source_link_id": link.id, "kind": kind_name(link.kind) }),
        )
        .await;
        log::source_link_event("source_link_added", link.id, kind_name(link.kind));
        let active = store::load_link(&self.pool, link.id).await?;
        if matches!(
            link.kind,
            SourceLinkKind::SharepointLocation | SourceLinkKind::ConfluenceLocation
        ) {
            if let Some(service) = self.me.upgrade() {
                let read = active.clone();
                self.spawn(async move { service.read_remote_link(read).await });
            }
        } else {
            self.changed(link.workspace_id).await;
        }
        Ok(active)
    }

    pub(crate) async fn add_membership(
        &self,
        ws: WorkspaceId,
        place: PlaceId,
    ) -> Result<(), WorkspaceError> {
        sqlx::query("INSERT OR IGNORE INTO workspace_places (workspace_id, place_id, added_at) VALUES (?1, ?2, ?3)")
            .bind(ws.to_string())
            .bind(place.to_string())
            .bind(Timestamp::now().to_wire())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The working copy of a remote link: folder, place, membership, adapter tracking, and for Jira the
    /// first read. Undone completely on a failure.
    async fn confirm_remote(&self, link: &SourceLink) -> Result<(), WorkspaceError> {
        let dir = self.working_copy_dir(link.id);
        let made = dir.clone();
        tokio::task::spawn_blocking(move || std::fs::create_dir_all(made))
            .await
            .map_err(|_| WorkspaceError::FilesFailed)?
            .map_err(|_| WorkspaceError::FilesFailed)?;
        let place = match self
            .files
            .add_place(AddPlace {
                path: dir.clone(),
                friendly_name: Some(link.display_name.clone()),
                kind: PlaceKind::WorkingCopy,
            })
            .await
        {
            Ok(place) => place.id,
            Err(FileError::PlaceExists { existing }) => existing,
            Err(error) => {
                self.remove_working_copy_dir(link.id).await;
                return Err(match error {
                    FileError::ProtectedLocation => {
                        WorkspaceError::PlaceRefused { cause: "protected" }
                    }
                    FileError::FolderUnreadable => WorkspaceError::PlaceRefused {
                        cause: "unreadable",
                    },
                    other => file_failed(other),
                });
            }
        };
        let stored = sqlx::query("UPDATE source_links SET place_id = ?2 WHERE id = ?1")
            .bind(link.id.to_string())
            .bind(place.to_string())
            .execute(&self.pool)
            .await;
        let outcome = match stored {
            Ok(_) => self.add_membership(link.workspace_id, place).await,
            Err(error) => Err(error.into()),
        };
        let outcome = match outcome {
            Ok(()) => {
                let mut placed = link.clone();
                placed.place_id = Some(place);
                self.track_new(&placed).await
            }
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            self.undo_remote(link, place).await;
            return Err(error);
        }
        Ok(())
    }

    /// Adapter tracking (SharePoint, Confluence) or the first Jira read (§J.2).
    async fn track_new(&self, link: &SourceLink) -> Result<(), WorkspaceError> {
        match link.kind {
            SourceLinkKind::SharepointLocation => self
                .sharepoint
                .track_link(link)
                .await
                .map_err(|e| map_adapter(&e)),
            SourceLinkKind::ConfluenceLocation => match &self.confluence {
                Some(confluence) => confluence
                    .track_link(link)
                    .await
                    .map_err(|e| map_adapter(&e)),
                None => Err(WorkspaceError::ConfluenceUnavailable),
            },
            SourceLinkKind::JiraIssue => self.jira_first_read(link).await,
            _ => Ok(()),
        }
    }

    async fn undo_remote(&self, link: &SourceLink, place: PlaceId) {
        match link.kind {
            SourceLinkKind::SharepointLocation => {
                if let Err(error) = self.sharepoint.untrack_link(link.id).await {
                    log::rollback_failed("untrack_link", error.code());
                }
            }
            SourceLinkKind::ConfluenceLocation => {
                if let Some(confluence) = &self.confluence {
                    if let Err(error) = confluence.untrack_link(link.id).await {
                        log::rollback_failed("untrack_link", error.code());
                    }
                }
            }
            _ => {}
        }
        let id = link.id.to_string();
        let _ = sqlx::query("DELETE FROM working_copy_unread WHERE source_link_id = ?1")
            .bind(&id)
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("DELETE FROM working_copy_items WHERE source_link_id = ?1")
            .bind(&id)
            .execute(&self.pool)
            .await;
        let _ =
            sqlx::query("DELETE FROM workspace_places WHERE workspace_id = ?1 AND place_id = ?2")
                .bind(link.workspace_id.to_string())
                .bind(place.to_string())
                .execute(&self.pool)
                .await;
        let _ = sqlx::query(
            "UPDATE source_links SET place_id = NULL, last_read_at = NULL WHERE id = ?1",
        )
        .bind(&id)
        .execute(&self.pool)
        .await;
        let _ = self.files.remove_place(place).await;
        self.remove_working_copy_dir(link.id).await;
        lock(&self.progress).remove(&link.id);
    }

    /// Internal Logic 3.4: `proposed` only; the row goes and nothing else.
    pub async fn decline_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError> {
        let link = store::load_link(&self.pool, link).await?;
        if link.state != SourceLinkState::Proposed {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        sqlx::query("DELETE FROM source_links WHERE id = ?1")
            .bind(link.id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// **Add** on the Add to this workspace screen: propose and confirm in one step. When the
    /// confirmation of a SharePoint or Confluence link fails, no link remains (Gherkin *SharePoint
    /// needs a sign-in*); a Jira link stays `proposed` (Gherkin *Jira not connected*) (WorkspaceService.md, Internal Logic 3).
    pub async fn add_link(
        &self,
        id: WorkspaceId,
        location: LinkLocation,
        display_name: Option<String>,
    ) -> Result<SourceLink, WorkspaceError> {
        // WS-ADD-1: a link left `proposed` by an earlier add (Jira not connected yet) is confirmed now.
        // spec: ordin-desktop/components/WorkspaceService#WS-ADD-1
        if let Some(waiting) = self.proposed_same(id, location.clone()).await? {
            return self.confirm_link(waiting).await;
        }
        let proposed = self.propose_link(id, location, display_name).await?;
        match self.confirm_link(proposed.id).await {
            Ok(link) => Ok(link),
            Err(error) => {
                if matches!(
                    proposed.kind,
                    SourceLinkKind::SharepointLocation | SourceLinkKind::ConfluenceLocation
                ) {
                    if let Err(error) = self.decline_link(proposed.id).await {
                        log::rollback_failed("decline_link", error.code());
                    }
                }
                Err(error)
            }
        }
    }

    // ---- 5. Removing a link

    /// Internal Logic 5.
    pub async fn remove_link(&self, link: SourceLinkId) -> Result<(), WorkspaceError> {
        let link = store::load_link(&self.pool, link).await?;
        if link.state != SourceLinkState::Active {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        let ws = link.workspace_id;
        // 5.2: the grants on its place, republished before anything is destroyed.
        let held = match link.place_id {
            Some(place) => {
                let rows = sqlx::query(
                    "SELECT * FROM folder_grants WHERE workspace_id = ?1 AND place_id = ?2",
                )
                .bind(ws.to_string())
                .bind(place.to_string())
                .fetch_all(&self.pool)
                .await?;
                rows.iter()
                    .map(crate::grants::grant_row)
                    .collect::<Result<Vec<_>, _>>()?
            }
            None => Vec::new(),
        };
        if !held.is_empty() {
            self.delete_grants_on(ws, link.place_id).await?;
            if let Err(error) = self.publish(ws).await {
                for grant in &held {
                    if let Err(error) = self.restore_grant(grant).await {
                        log::rollback_failed("restore_grant", error.code());
                    }
                }
                if let Err(error) = self.publish(ws).await {
                    log::rollback_failed("publish", error.code());
                }
                return Err(error);
            }
        }
        // 5.3 and 5.4
        match link.kind {
            SourceLinkKind::LocalFolder | SourceLinkKind::GitRepositoryFolder => {
                if let Some(place) = link.place_id {
                    self.release_folder_place(&link, place).await?;
                }
            }
            SourceLinkKind::SharepointLocation
            | SourceLinkKind::ConfluenceLocation
            | SourceLinkKind::JiraIssue => {
                self.untrack(&link).await;
                if let Some(place) = link.place_id {
                    if self.files.remove_place(place).await.is_err() {
                        log::background_failed("remove_place", "WS_FILES_FAILED");
                    }
                    sqlx::query(
                        "DELETE FROM workspace_places WHERE workspace_id = ?1 AND place_id = ?2",
                    )
                    .bind(ws.to_string())
                    .bind(place.to_string())
                    .execute(&self.pool)
                    .await?;
                }
                self.remove_working_copy_dir(link.id).await;
            }
            SourceLinkKind::GitRemote => {}
        }
        // 5.5
        let id = link.id.to_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM working_copy_unread WHERE source_link_id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM working_copy_items WHERE source_link_id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM source_links WHERE id = ?1")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        lock(&self.progress).remove(&link.id);
        self.record(
            "source_link_removed",
            Some(ws),
            "A location was removed from a workspace.",
            serde_json::json!({ "source_link_id": link.id, "kind": kind_name(link.kind) }),
        )
        .await;
        log::source_link_event("source_link_removed", link.id, kind_name(link.kind));
        // 5.6 (ADR-103): a send with no receiver is not an error.
        let _ = self.removed_tx.send((ws, link.id));
        self.changed(ws).await;
        Ok(())
    }

    /// A local or git folder: the membership goes; the place goes when no other workspace, no other
    /// link of this workspace and no own-folder role holds it. The folder is untouched.
    async fn release_folder_place(
        &self,
        link: &SourceLink,
        place: PlaceId,
    ) -> Result<(), WorkspaceError> {
        let ws = link.workspace_id;
        let own = self.workspace(ws).await?.own_folder_place_id;
        let shared = sqlx::query(
            "SELECT count(*) AS n FROM source_links WHERE workspace_id = ?1 AND place_id = ?2 AND id != ?3",
        )
        .bind(ws.to_string())
        .bind(place.to_string())
        .bind(link.id.to_string())
        .fetch_one(&self.pool)
        .await?
        .try_get::<i64, _>("n")?;
        if shared > 0 || own == place {
            return Ok(());
        }
        sqlx::query("DELETE FROM workspace_places WHERE workspace_id = ?1 AND place_id = ?2")
            .bind(ws.to_string())
            .bind(place.to_string())
            .execute(&self.pool)
            .await?;
        let elsewhere =
            sqlx::query("SELECT count(*) AS n FROM workspace_places WHERE place_id = ?1")
                .bind(place.to_string())
                .fetch_one(&self.pool)
                .await?
                .try_get::<i64, _>("n")?;
        if elsewhere == 0 {
            self.files.unwatch(&[place]).await;
            if self.files.remove_place(place).await.is_err() {
                log::background_failed("remove_place", "WS_FILES_FAILED");
            }
        }
        Ok(())
    }

    /// `untrack_link` at the adapter of a remote link; a failure is logged, never a stop.
    pub(crate) async fn untrack(&self, link: &SourceLink) {
        match link.kind {
            SourceLinkKind::SharepointLocation => {
                if let Err(error) = self.sharepoint.untrack_link(link.id).await {
                    log::background_failed("untrack_link", error.code());
                }
            }
            SourceLinkKind::ConfluenceLocation => {
                if let Some(confluence) = &self.confluence {
                    if let Err(error) = confluence.untrack_link(link.id).await {
                        log::background_failed("untrack_link", error.code());
                    }
                }
            }
            _ => {}
        }
    }

    /// Internal Logic 4: how far the background read of a remote link has come. A folder on this
    /// computer is read where it is and never into a working copy, so it is done at once (WS-LP-1).
    // spec: ordin-desktop/components/WorkspaceService#WS-LP-1
    pub async fn link_progress(&self, link: SourceLinkId) -> Result<LinkProgress, WorkspaceError> {
        let stored = store::load_link(&self.pool, link).await?;
        if matches!(
            stored.kind,
            SourceLinkKind::LocalFolder | SourceLinkKind::GitRepositoryFolder
        ) {
            return Ok(LinkProgress {
                source_link_id: link,
                listed: 0,
                read: 0,
                not_read: 0,
                done: true,
            });
        }
        if let Some(progress) = lock(&self.progress).get(&link).cloned() {
            return Ok(progress);
        }
        let read = self
            .count(
                "SELECT count(*) AS n FROM working_copy_items WHERE source_link_id = ?1",
                link,
            )
            .await?;
        let not_read = self
            .count(
                "SELECT count(*) AS n FROM working_copy_unread WHERE source_link_id = ?1",
                link,
            )
            .await?;
        Ok(LinkProgress {
            source_link_id: link,
            listed: read + not_read,
            read,
            not_read,
            done: stored.last_read_at.is_some() || stored.state == SourceLinkState::Proposed,
        })
    }

    async fn count(&self, sql: &'static str, link: SourceLinkId) -> Result<u64, WorkspaceError> {
        let n = sqlx::query(sql)
            .bind(link.to_string())
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("n")?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}
