//! A team workspace (WorkspaceService.md amendment January 2026, ADR-121): the results folder and
//! the record of which run wrote which result (WS-TW-1 to WS-TW-4), the waiting grants (WS-TW-5),
//! links made from the places a team workspace description names (WS-TW-6), the end of a team
//! workspace (WS-TW-7) and the result path rule (WS-TW-9). The Team Knowledge Service owns the team
//! folder and calls these methods through the program; this module only performs the local steps.

use std::collections::HashSet;
use std::path::PathBuf;

use ordin_files::{AddPlace, FileError, Place, PlaceChange, PlaceChangeKind, PlaceKind};
use ordin_types::domain::places::{FileVersionOrigin, FolderAccess, SourceLink, SourceLinkKind};
use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{AgentInstanceId, PlaceId, RunId, TeamGrantId, WorkspaceId};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::{file_failed, WorkspaceError};
use crate::links::{map_adapter, parse_git_remote};
use crate::log;
use crate::service::{lock, WorkspaceService};
use crate::store::{self, from_text, to_text};
use crate::types::LinkLocation;
use crate::SAVED_FILE_MAX_BYTES;

/// `<documents>/ordin/Linked copies/results-<WorkspaceId>/`, inside the folder WS-WC-1 reserves.
pub const TEAM_RESULTS_DIR_PREFIX: &str = "results-";
/// The results folder's friendly name, as the tree and the File Service show it.
pub const TEAM_RESULTS_NAME: &str = "Team results";

/// How a place a team workspace description names can be reached with this person's sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    Reachable,
    NoAccess,
    SignInNeeded,
    NotConfigured,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OriginReach {
    pub reach: Reach,
}

/// One file of the results folder as this service records it (WS-TW-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamResultFile {
    /// Relative to the results folder.
    pub path: String,
    /// The run that wrote its current version; `None` for the person's own edit.
    pub run_id: Option<RunId>,
    /// The File Service version.
    pub version: u32,
    pub sha256: Sha256Hex,
    pub size_bytes: u64,
}

/// One place a waiting grant asks for, and the access it asks for there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingAccess {
    /// The description's place key, or "own", or "results".
    pub place_key: String,
    /// `None` when this person cannot open the place.
    pub place_id: Option<PlaceId>,
    /// 1–120 characters.
    pub place_name: String,
    pub access: FolderAccess,
}

/// An agent or access a colleague added to the team workspace, as the Team Knowledge Service hands
/// it over (WS-TW-5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NewPendingGrant {
    pub workspace_id: WorkspaceId,
    /// The description's agent key (UUIDv7).
    pub agent_key: String,
    /// 1–120 characters.
    pub agent_name: String,
    pub repository: AgentRepositoryRef,
    /// 1–300 characters.
    pub agent_id: String,
    /// 1–64 characters.
    pub agent_version: String,
    /// `Some` when the agent is already in this workspace.
    pub agent_instance_id: Option<AgentInstanceId>,
    /// 1–102 entries.
    pub access: Vec<PendingAccess>,
    /// A login, 1–100 characters.
    pub added_by: String,
    /// false: the repository's history does not record `added_by` as the author
    /// (TeamKnowledgeService.md TW-J); the screens and Needs you say "says it was".
    pub added_by_confirmed: bool,
    pub added_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingGrant {
    pub id: TeamGrantId,
    #[serde(flatten)]
    pub grant: NewPendingGrant,
    pub state: PendingGrantState,
    pub decided_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingGrantState {
    Waiting,
    Allowed,
    Declined,
    Replaced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantDecision {
    Allowed,
    Declined,
}

/// The result path rule (WS-TW-9), one rule for the results folder on every computer and for
/// `results/` of the team folder, so a path one computer writes is a path every computer, on macOS,
/// Linux and Windows, can store. `WS_RESULT_PATH_INVALID` for any other path. The Team Knowledge
/// Service applies the same function to the paths of activity files and of `results/`.
// spec: ordin-desktop/components/WorkspaceService#WS-TW-9
pub fn check_result_path(path: &str) -> Result<(), WorkspaceError> {
    let invalid = Err(WorkspaceError::ResultPathInvalid);
    if path.is_empty() || path.len() > 1024 || path.starts_with('/') {
        return invalid;
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment.len() > 255
            || segment.starts_with('.')
            || segment.ends_with(' ')
            || segment.ends_with('.')
        {
            return invalid;
        }
        // `char::is_control` is exactly Unicode category Cc, NUL included.
        if segment
            .chars()
            .any(|c| matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control())
        {
            return invalid;
        }
        let stem = segment.split('.').next().unwrap_or(segment);
        if is_windows_reserved(stem) {
            return invalid;
        }
    }
    Ok(())
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9` and `LPT1` to `LPT9`, compared case-insensitively.
fn is_windows_reserved(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && (b'1'..=b'9').contains(&bytes[3])
}

/// One of the four origin forms of WS-TW-6, parsed.
enum OriginForm {
    Sharepoint,
    Confluence,
    Git {
        url: String,
        branch: String,
        path: String,
    },
    Jira(ordin_jira::JiraIssueRef),
}

/// `graph:drives/<drive>/items/<item>`: a drive and an item id of the characters Graph uses, so no
/// user part, query or fragment.
fn sharepoint_form(origin: &str) -> bool {
    let id_ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '!' | '-' | '_' | '.' | '~'))
    };
    origin
        .strip_prefix("graph:drives/")
        .and_then(|rest| rest.split_once("/items/"))
        .is_some_and(|(drive, item)| id_ok(drive) && id_ok(item))
}

/// The Confluence Adapter's origin forms, `confluence:page/<id>` and `confluence:space/<key>`.
fn confluence_form(origin: &str) -> bool {
    if let Some(id) = origin.strip_prefix("confluence:page/") {
        return !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_digit());
    }
    origin.strip_prefix("confluence:space/").is_some_and(|key| {
        !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '~' | '-' | '_' | '.'))
    })
}

/// `git:<url>#<branch>:<path>`, split at the `#` and the first `:` after it; the parts must make the
/// same origin back (SA-CLOSE-03), and an `https` address must carry no user part.
fn git_form(origin: &str) -> Option<OriginForm> {
    let rest = origin.strip_prefix("git:")?;
    let (url, tail) = rest.split_once('#')?;
    let (branch, path) = tail.split_once(':')?;
    if !url.starts_with("git@") {
        let parsed = url::Url::parse(url).ok()?;
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return None;
        }
    }
    let (canonical, _) = parse_git_remote(url, branch, path).ok()?;
    (canonical == origin).then(|| OriginForm::Git {
        url: url.to_owned(),
        branch: branch.to_owned(),
        path: path.to_owned(),
    })
}

/// `WS_ORIGIN_INVALID` for any other kind, or a string that is not its kind's form.
fn origin_form(kind: SourceLinkKind, origin: &str) -> Result<OriginForm, WorkspaceError> {
    let invalid = WorkspaceError::OriginInvalid;
    if origin.is_empty() || origin.len() > 2048 || origin.chars().any(char::is_control) {
        return Err(invalid);
    }
    match kind {
        SourceLinkKind::SharepointLocation if sharepoint_form(origin) => Ok(OriginForm::Sharepoint),
        SourceLinkKind::ConfluenceLocation if confluence_form(origin) => Ok(OriginForm::Confluence),
        SourceLinkKind::GitRemote => git_form(origin).ok_or(invalid),
        SourceLinkKind::JiraIssue => {
            let issue = ordin_jira::JiraAdapter::parse_issue_address(origin)
                .map_err(|_| invalid.clone())?;
            if ordin_jira::JiraAdapter::issue_address(&issue) == origin {
                Ok(OriginForm::Jira(issue))
            } else {
                Err(invalid)
            }
        }
        _ => Err(invalid),
    }
}

/// WS-TW-6 step 1: the meaning of an adapter's answer.
fn reach_of(outcome: Result<(), WorkspaceError>) -> Result<Reach, WorkspaceError> {
    Ok(match outcome {
        Ok(()) => Reach::Reachable,
        Err(WorkspaceError::SourceAccessDenied | WorkspaceError::SourceNotFound) => Reach::NoAccess,
        Err(WorkspaceError::SourceSignInNeeded) => Reach::SignInNeeded,
        Err(WorkspaceError::SourceNotConfigured) => Reach::NotConfigured,
        // The form was checked first, so an adapter that still refuses the location refuses the form.
        Err(WorkspaceError::Invalid("SRC_LOCATION_INVALID")) => {
            return Err(WorkspaceError::OriginInvalid)
        }
        // SPEC-GAP-1: WS-TW-6 maps `*_UNAVAILABLE` and `*_THROTTLED` to `Unavailable` and names no
        // reach for the adapters' other codes (`*_TOO_LARGE`, `*_SITE_MISMATCH`,
        // `*_RESPONSE_INVALID`); they are read as `Unavailable` here, as §3.5 reads them as
        // `WS_SOURCE_UNAVAILABLE`.
        Err(_) => Reach::Unavailable,
    })
}

fn text_ok(text: &str, max_chars: usize) -> bool {
    let chars = text.chars().count();
    (1..=max_chars).contains(&chars) && !text.trim().is_empty()
}

/// WS-TW-5: the field bounds the type writes.
// SPEC-GAP-2: WS-TW-5 gives `Invalid("SRC_NAME_INVALID")` for a name out of bounds and no code for
// the other bounds of `NewPendingGrant` (`agent_key`, `agent_id`, `agent_version`, the number of
// `access` entries, `added_by`); every bound refuses with `SRC_NAME_INVALID` here.
fn check_pending_grant(g: &NewPendingGrant) -> Result<(), WorkspaceError> {
    let invalid = Err(WorkspaceError::Invalid("SRC_NAME_INVALID"));
    let key_ok = g.agent_key.chars().count() == 36 && uuid::Uuid::parse_str(&g.agent_key).is_ok();
    if !key_ok
        || !text_ok(&g.agent_name, 120)
        || !text_ok(&g.agent_id, 300)
        || !text_ok(&g.agent_version, 64)
        || !text_ok(&g.added_by, 100)
        || g.access.is_empty()
        || g.access.len() > 102
        || g.access.iter().any(|a| !text_ok(&a.place_name, 120))
    {
        return invalid;
    }
    Ok(())
}

fn result_row(row: &SqliteRow) -> Result<TeamResultFile, WorkspaceError> {
    let version: i64 = row.try_get("version")?;
    let size: i64 = row.try_get("size_bytes")?;
    Ok(TeamResultFile {
        path: row.try_get("path")?,
        run_id: store::id_opt(row, "run_id")?,
        version: u32::try_from(version).map_err(|_| WorkspaceError::DatastoreUnavailable)?,
        sha256: Sha256Hex(row.try_get("sha256")?),
        size_bytes: u64::try_from(size).map_err(|_| WorkspaceError::DatastoreUnavailable)?,
    })
}

fn grant_row(row: &SqliteRow) -> Result<PendingGrant, WorkspaceError> {
    let damaged = |_| WorkspaceError::DatastoreUnavailable;
    let repository: AgentRepositoryRef =
        serde_json::from_str(&row.try_get::<String, _>("repository")?).map_err(damaged)?;
    let access: Vec<PendingAccess> =
        serde_json::from_str(&row.try_get::<String, _>("access")?).map_err(damaged)?;
    let confirmed: i64 = row.try_get("added_by_confirmed")?;
    Ok(PendingGrant {
        id: store::id_of(row, "id")?,
        grant: NewPendingGrant {
            workspace_id: store::id_of(row, "workspace_id")?,
            agent_key: row.try_get("agent_key")?,
            agent_name: row.try_get("agent_name")?,
            repository,
            agent_id: row.try_get("agent_id")?,
            agent_version: row.try_get("agent_version")?,
            agent_instance_id: store::id_opt(row, "agent_instance_id")?,
            access,
            added_by: row.try_get("added_by")?,
            added_by_confirmed: confirmed == 1,
            added_at: store::ts_req(row, "added_at")?,
        },
        state: from_text(&row.try_get::<String, _>("state")?)?,
        decided_at: store::ts_opt(row, "decided_at")?,
    })
}

fn byte_count(bytes: usize) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

impl WorkspaceService {
    // ---- WS-TW-6 Places named by a description

    /// WS-TW-6 step 1: whether this person can reach a place a description names. Nothing is stored.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-6
    pub async fn check_origin(
        &self,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<OriginReach, WorkspaceError> {
        let form = origin_form(kind, origin_location)?;
        let outcome = match form {
            OriginForm::Sharepoint => {
                match self.sharepoint.resolve_origin(origin_location).await {
                    // A file has no children to list: reading it is the check.
                    Ok(loc) if !loc.is_folder => Ok(()),
                    Ok(loc) => self
                        .sharepoint
                        .list_children(&loc, None)
                        .await
                        .map(|_| ())
                        .map_err(|e| map_adapter(&e)),
                    Err(error) => Err(map_adapter(&error)),
                }
            }
            OriginForm::Confluence => {
                let Some(confluence) = &self.confluence else {
                    return Ok(OriginReach {
                        reach: Reach::NotConfigured,
                    });
                };
                match confluence.resolve_origin(origin_location).await {
                    Ok(loc) => confluence
                        .list_children(&loc, None)
                        .await
                        .map(|_| ())
                        .map_err(|e| map_adapter(&e)),
                    Err(error) => Err(map_adapter(&error)),
                }
            }
            // The Team Knowledge Service checks repository access itself.
            OriginForm::Git { .. } => Ok(()),
            OriginForm::Jira(issue) => {
                let Some(jira) = &self.jira else {
                    return Ok(OriginReach {
                        reach: Reach::NotConfigured,
                    });
                };
                jira.issue(&issue)
                    .await
                    .map(|_| ())
                    .map_err(|e| map_adapter(&e))
            }
        };
        Ok(OriginReach {
            reach: reach_of(outcome)?,
        })
    }

    /// WS-TW-6 step 2: links a place a description names with this person's own sign-in, through
    /// `add_link` and every rule of §3. An existing link of the same kind and origin is returned.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-6
    pub async fn add_link_from_origin(
        &self,
        id: WorkspaceId,
        kind: SourceLinkKind,
        origin_location: &str,
        display_name: &str,
    ) -> Result<SourceLink, WorkspaceError> {
        origin_form(kind, origin_location)?;
        self.ensure_active(id).await?;
        if let Some(existing) = self.active_link_at(id, kind, origin_location).await? {
            return Ok(existing);
        }
        let location = self.resolve_origin(kind, origin_location).await?;
        self.add_link(id, location, Some(display_name.to_owned()))
            .await
    }

    /// WS-TW-6 step 3 (amended January 2026, the user's decision): the location an origin names,
    /// read with this person's sign-in, as step 2 builds it; nothing is linked or stored.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-6
    pub async fn resolve_origin(
        &self,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<LinkLocation, WorkspaceError> {
        let form = origin_form(kind, origin_location)?;
        Ok(match form {
            OriginForm::Sharepoint => LinkLocation::Sharepoint {
                location: self
                    .sharepoint
                    .resolve_origin(origin_location)
                    .await
                    .map_err(|e| map_adapter(&e))?,
            },
            OriginForm::Confluence => {
                let confluence = self
                    .confluence
                    .as_ref()
                    .ok_or(WorkspaceError::ConfluenceUnavailable)?;
                LinkLocation::Confluence {
                    location: confluence
                        .resolve_origin(origin_location)
                        .await
                        .map_err(|e| map_adapter(&e))?,
                }
            }
            OriginForm::Git { url, branch, path } => LinkLocation::GitRemote { url, branch, path },
            OriginForm::Jira(_) => LinkLocation::JiraIssue {
                address: origin_location.to_owned(),
            },
        })
    }

    /// The workspace's `active` link of this kind and origin, if any (a `proposed` one is confirmed
    /// by `add_link`, WS-ADD-1).
    async fn active_link_at(
        &self,
        id: WorkspaceId,
        kind: SourceLinkKind,
        origin_location: &str,
    ) -> Result<Option<SourceLink>, WorkspaceError> {
        let row = sqlx::query(
            "SELECT * FROM source_links WHERE workspace_id = ?1 AND kind = ?2 AND origin_location = ?3 \
             AND state = 'active'",
        )
        .bind(id.to_string())
        .bind(to_text(&kind))
        .bind(origin_location)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(store::link_row).transpose()
    }

    // ---- WS-TW-1 The results folder

    /// WS-TW-1: the results folder of an active workspace, made once; a second call returns it.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-1
    pub async fn make_results_folder(&self, id: WorkspaceId) -> Result<Place, WorkspaceError> {
        self.ensure_active(id).await?;
        if let Some(place) = self.live_results_place(id).await? {
            return self.files.place(place).await.map_err(file_failed);
        }
        let folder = self
            .linked_copies_root()
            .join(format!("{TEAM_RESULTS_DIR_PREFIX}{id}"));
        let made = folder.clone();
        let created_here = tokio::task::spawn_blocking(move || {
            let existed = made.is_dir();
            std::fs::create_dir_all(&made).map(|()| !existed)
        })
        .await
        .map_err(|_| WorkspaceError::FilesFailed)?
        .map_err(|_| WorkspaceError::FilesFailed)?;
        if self.rules.is_protected_location(&folder) {
            remove_if_made(folder, created_here).await;
            // SPEC-GAP-3: WS-TW-1 requires `rules.is_protected_location` to be false and names no
            // code when it is not; `WS_PLACE_REFUSED` (cause "protected") is used, as for a folder
            // link (§3.1).
            return Err(WorkspaceError::PlaceRefused { cause: "protected" });
        }
        let place = match self
            .files
            .add_place(AddPlace {
                path: folder.clone(),
                friendly_name: Some(TEAM_RESULTS_NAME.to_owned()),
                kind: PlaceKind::WorkingCopy,
            })
            .await
        {
            Ok(place) => place,
            Err(FileError::PlaceExists { existing }) => {
                self.files.place(existing).await.map_err(file_failed)?
            }
            Err(error) => {
                remove_if_made(folder, created_here).await;
                return Err(file_failed(error));
            }
        };
        let now = Timestamp::now().to_wire();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO workspace_places (workspace_id, place_id, added_at) VALUES (?1, ?2, ?3)",
        )
        .bind(id.to_string())
        .bind(place.id.to_string())
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        // SPEC-GAP-4: WS-TW-1 does not say what a second team period does with the row an earlier
        // `end_team` left (`ended_at` set, one row per workspace); the row is made live again on the
        // same folder.
        sqlx::query(
            "INSERT INTO team_results_folders (workspace_id, place_id, made_at, ended_at) VALUES (?1, ?2, ?3, NULL) \
             ON CONFLICT (workspace_id) DO UPDATE SET place_id = excluded.place_id, made_at = excluded.made_at, ended_at = NULL",
        )
        .bind(id.to_string())
        .bind(place.id.to_string())
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Err(error) = self.files.watch(&[place.id]).await {
            log::background_failed("watch", error.code());
        }
        self.record(
            "results_folder_made",
            Some(id),
            "A results folder was made for a team workspace.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("results_folder_made", id);
        self.changed(id).await;
        Ok(place)
    }

    /// WS-TW-1: the results folder's place while the workspace is a team workspace, else `None`.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-1
    pub async fn results_folder(&self, id: WorkspaceId) -> Result<Option<PlaceId>, WorkspaceError> {
        self.workspace(id).await?;
        self.live_results_place(id).await
    }

    pub(crate) async fn live_results_place(
        &self,
        id: WorkspaceId,
    ) -> Result<Option<PlaceId>, WorkspaceError> {
        let row = sqlx::query(
            "SELECT place_id FROM team_results_folders WHERE workspace_id = ?1 AND ended_at IS NULL",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref()
            .map(|r| store::id_of(r, "place_id"))
            .transpose()
    }

    /// The results folder of a workspace that has one (`WS_NOT_FOUND`, `WS_NOT_TEAM`).
    async fn results_place(&self, id: WorkspaceId) -> Result<PlaceId, WorkspaceError> {
        self.workspace(id).await?;
        self.live_results_place(id)
            .await?
            .ok_or(WorkspaceError::NotTeam)
    }

    // ---- WS-TW-3 Which run wrote which result

    /// WS-TW-3 step 3: the paths the run wrote, in path order, at most 50.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-3
    pub async fn results_of_run(
        &self,
        id: WorkspaceId,
        run_id: RunId,
    ) -> Result<Vec<String>, WorkspaceError> {
        self.results_place(id).await?;
        let rows = sqlx::query(
            "SELECT path FROM team_result_files WHERE workspace_id = ?1 AND run_id = ?2 ORDER BY path LIMIT 50",
        )
        .bind(id.to_string())
        .bind(run_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| r.try_get::<String, _>("path").map_err(Into::into))
            .collect()
    }

    /// WS-TW-3 step 3: the results not sent as they are, in path order. A path that breaks WS-TW-9
    /// is never listed, so it stays on this computer.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-3
    pub async fn unsent_results(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<TeamResultFile>, WorkspaceError> {
        self.results_place(id).await?;
        let rows = sqlx::query(
            "SELECT * FROM team_result_files WHERE workspace_id = ?1 \
             AND (sent_sha256 IS NULL OR sent_sha256 != sha256) ORDER BY path",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut unsent = Vec::with_capacity(rows.len());
        for row in &rows {
            let file = result_row(row)?;
            if check_result_path(&file.path).is_ok() {
                unsent.push(file);
            }
        }
        Ok(unsent)
    }

    /// WS-TW-3 step 3: the bytes of one result.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-3
    pub async fn read_result(
        &self,
        id: WorkspaceId,
        path: &str,
    ) -> Result<Vec<u8>, WorkspaceError> {
        check_result_path(path)?;
        let place = self.results_place(id).await?;
        Ok(self
            .files
            .read_file(place, path)
            .await
            .map_err(file_failed)?
            .bytes)
    }

    /// WS-TW-3 step 3: records what was sent, per path.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-3
    pub async fn mark_results_sent(
        &self,
        id: WorkspaceId,
        sent: Vec<(String, Sha256Hex)>,
    ) -> Result<(), WorkspaceError> {
        for (path, _) in &sent {
            check_result_path(path)?;
        }
        self.results_place(id).await?;
        let mut tx = self.pool.begin().await?;
        for (path, hash) in &sent {
            sqlx::query(
                "UPDATE team_result_files SET sent_sha256 = ?3 WHERE workspace_id = ?1 AND path = ?2",
            )
            .bind(id.to_string())
            .bind(path)
            .bind(&hash.0)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// WS-TW-3 steps 1 and 2: a File Service change on a results folder's place.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-3
    pub(crate) async fn results_place_change(&self, change: &PlaceChange) {
        let rows = sqlx::query(
            "SELECT workspace_id FROM team_results_folders WHERE place_id = ?1 AND ended_at IS NULL",
        )
        .bind(change.place_id.to_string())
        .fetch_all(&self.pool)
        .await;
        let Ok(rows) = rows else { return };
        for row in &rows {
            let Ok(ws) = store::id_of::<WorkspaceId>(row, "workspace_id") else {
                continue;
            };
            if let Err(error) = self.result_change(ws, change).await {
                log::background_failed("result_change", error.code());
            }
        }
    }

    async fn result_change(
        &self,
        ws: WorkspaceId,
        change: &PlaceChange,
    ) -> Result<(), WorkspaceError> {
        match &change.change {
            PlaceChangeKind::Created | PlaceChangeKind::Modified => {
                self.record_result(ws, change).await
            }
            PlaceChangeKind::Removed => {
                sqlx::query("DELETE FROM team_result_files WHERE workspace_id = ?1 AND path = ?2")
                    .bind(ws.to_string())
                    .bind(&change.path)
                    .execute(&self.pool)
                    .await?;
                self.changed(ws).await;
                Ok(())
            }
            PlaceChangeKind::Renamed { from } => {
                let held = sqlx::query(
                    "SELECT count(*) AS n FROM team_result_files WHERE workspace_id = ?1 AND path = ?2",
                )
                .bind(ws.to_string())
                .bind(from)
                .fetch_one(&self.pool)
                .await?
                .try_get::<i64, _>("n")?;
                // SPEC-GAP-5: WS-TW-3 step 2 moves the row at `from` to `path` keeping `run_id` and
                // says nothing of its other columns or of a rename whose `from` has no row; the row
                // keeps every column, and a rename with no row at `from` is recorded as a new file.
                if held == 0 {
                    return self.record_result(ws, change).await;
                }
                let mut tx = self.pool.begin().await?;
                sqlx::query("DELETE FROM team_result_files WHERE workspace_id = ?1 AND path = ?2")
                    .bind(ws.to_string())
                    .bind(&change.path)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "UPDATE team_result_files SET path = ?3 WHERE workspace_id = ?1 AND path = ?2",
                )
                .bind(ws.to_string())
                .bind(from)
                .bind(&change.path)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                self.changed(ws).await;
                Ok(())
            }
        }
    }

    /// WS-TW-3 step 1: the current version of a created or changed file, with the run that wrote it.
    async fn record_result(
        &self,
        ws: WorkspaceId,
        change: &PlaceChange,
    ) -> Result<(), WorkspaceError> {
        let place = change.place_id;
        let path = change.path.as_str();
        let current = self
            .files
            .current_version(place, path)
            .await
            .map_err(file_failed)?;
        let (hash, version, run_id, size) = match current {
            Some(found) => {
                let run_id = match found.origin {
                    FileVersionOrigin::Run { run_id, .. } => Some(run_id),
                    _ => None,
                };
                let size = self.result_size(place, path).await?;
                (found.hash, found.version, run_id, size)
            }
            // SPEC-GAP-6: WS-TW-3 reads the version from `files.current_version`, which has none for a
            // file the person made in another app (the File Service keeps no version for it); its
            // hash and size come from `read_file` and it is recorded as version 1 with no run.
            None => {
                let bytes = self
                    .files
                    .read_file(place, path)
                    .await
                    .map_err(file_failed)?;
                let size = bytes.bytes.len() as u64;
                (bytes.hash, 1, None, size)
            }
        };
        let synced: Option<String> = sqlx::query(
            "SELECT synced_sha256 FROM team_result_files WHERE workspace_id = ?1 AND path = ?2",
        )
        .bind(ws.to_string())
        .bind(path)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| r.try_get::<Option<String>, _>("synced_sha256"))
        .transpose()?
        .flatten();
        if synced.as_deref() == Some(hash.0.as_str()) {
            return Ok(());
        }
        if check_result_path(path).is_err()
            && lock(&self.results_not_shared).insert((ws, path.to_owned()))
        {
            log::workspace_event("result_path_not_shared", ws);
        }
        sqlx::query(
            "INSERT INTO team_result_files (workspace_id, path, run_id, version, sha256, size_bytes, \
             sent_sha256, synced_sha256, changed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7) \
             ON CONFLICT (workspace_id, path) DO UPDATE SET run_id = excluded.run_id, \
             version = excluded.version, sha256 = excluded.sha256, size_bytes = excluded.size_bytes, \
             changed_at = excluded.changed_at",
        )
        .bind(ws.to_string())
        .bind(path)
        .bind(run_id.map(|r| r.to_string()))
        .bind(i64::from(version))
        .bind(&hash.0)
        .bind(i64::try_from(size).unwrap_or(i64::MAX))
        .bind(change.observed_at.to_wire())
        .execute(&self.pool)
        .await?;
        self.changed(ws).await;
        Ok(())
    }

    /// The size of a file of the results folder, read from the folder itself.
    async fn result_size(&self, place: PlaceId, path: &str) -> Result<u64, WorkspaceError> {
        let root = PathBuf::from(self.files.place(place).await.map_err(file_failed)?.path);
        let target = root.join(path);
        tokio::task::spawn_blocking(move || std::fs::metadata(target).map(|m| m.len()))
            .await
            .map_err(|_| WorkspaceError::FilesFailed)?
            .map_err(|_| WorkspaceError::FilesFailed)
    }

    // ---- WS-TW-4 Results from the team

    /// WS-TW-4: stores a result from the team, recorded as sent and synced so it is neither sent back
    /// unchanged nor counted as this person's change.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-4
    pub async fn store_team_result(
        &self,
        id: WorkspaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Sha256Hex, WorkspaceError> {
        check_result_path(path)?;
        if bytes.len() as u64 > SAVED_FILE_MAX_BYTES {
            return Err(WorkspaceError::FileTooLarge);
        }
        let place = self.results_place(id).await?;
        self.store_synced(id, place, path, bytes).await
    }

    /// `files.store_fetched` and the row with `sha256`, `sent_sha256` and `synced_sha256` all the
    /// stored hash and no run.
    async fn store_synced(
        &self,
        id: WorkspaceId,
        place: PlaceId,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<Sha256Hex, WorkspaceError> {
        let size = byte_count(bytes.len());
        let now = Timestamp::now();
        let hash = self
            .files
            .store_fetched(place, path, bytes, None, now)
            .await
            .map_err(file_failed)?;
        let version = self.version_of(place, path).await?;
        sqlx::query(
            "INSERT INTO team_result_files (workspace_id, path, run_id, version, sha256, size_bytes, \
             sent_sha256, synced_sha256, changed_at) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?4, ?4, ?6) \
             ON CONFLICT (workspace_id, path) DO UPDATE SET run_id = NULL, version = excluded.version, \
             sha256 = excluded.sha256, size_bytes = excluded.size_bytes, sent_sha256 = excluded.sha256, \
             synced_sha256 = excluded.sha256, changed_at = excluded.changed_at",
        )
        .bind(id.to_string())
        .bind(path)
        .bind(i64::from(version))
        .bind(&hash.0)
        .bind(size)
        .bind(now.to_wire())
        .execute(&self.pool)
        .await?;
        Ok(hash)
    }

    /// The File Service's version of a file just stored (at least 1).
    async fn version_of(&self, place: PlaceId, path: &str) -> Result<u32, WorkspaceError> {
        Ok(self
            .files
            .current_version(place, path)
            .await
            .map_err(file_failed)?
            .map_or(1, |v| v.version.max(1)))
    }

    /// WS-TW-4: a result the team removed goes to ordin's Bin, from which the person can restore it.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-4
    pub async fn remove_team_result(
        &self,
        id: WorkspaceId,
        path: &str,
    ) -> Result<(), WorkspaceError> {
        check_result_path(path)?;
        let place = self.results_place(id).await?;
        self.files
            .move_to_bin(place, path)
            .await
            .map_err(file_failed)?;
        sqlx::query("DELETE FROM team_result_files WHERE workspace_id = ?1 AND path = ?2")
            .bind(id.to_string())
            .bind(path)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Two results of one name (WS-TW-4): `theirs` at `path`, this person's version moved to `mine_to`.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-4
    pub async fn keep_both_results(
        &self,
        id: WorkspaceId,
        path: &str,
        theirs: Vec<u8>,
        mine_to: &str,
    ) -> Result<Sha256Hex, WorkspaceError> {
        check_result_path(path)?;
        check_result_path(mine_to)?;
        if theirs.len() as u64 > SAVED_FILE_MAX_BYTES {
            return Err(WorkspaceError::FileTooLarge);
        }
        let place = self.results_place(id).await?;
        if self.mine_to_taken(id, place, mine_to).await? {
            return Err(WorkspaceError::ResultPathTaken);
        }
        let mine = self
            .files
            .read_file(place, path)
            .await
            .map_err(file_failed)?
            .bytes;
        let run_id: Option<String> = sqlx::query(
            "SELECT run_id FROM team_result_files WHERE workspace_id = ?1 AND path = ?2",
        )
        .bind(id.to_string())
        .bind(path)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| r.try_get::<Option<String>, _>("run_id"))
        .transpose()?
        .flatten();
        // This person's version first, unsent and with its run, so nothing of theirs is lost if the
        // second write fails.
        let size = byte_count(mine.len());
        let now = Timestamp::now();
        let mine_hash = self
            .files
            .store_fetched(place, mine_to, mine, None, now)
            .await
            .map_err(file_failed)?;
        let version = self.version_of(place, mine_to).await?;
        sqlx::query(
            "INSERT INTO team_result_files (workspace_id, path, run_id, version, sha256, size_bytes, \
             sent_sha256, synced_sha256, changed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7)",
        )
        .bind(id.to_string())
        .bind(mine_to)
        .bind(run_id)
        .bind(i64::from(version))
        .bind(&mine_hash.0)
        .bind(size)
        .bind(now.to_wire())
        .execute(&self.pool)
        .await?;
        let hash = self.store_synced(id, place, path, theirs).await?;
        self.record(
            "team_results_both_kept",
            Some(id),
            "Two results of one name were both kept.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        self.changed(id).await;
        Ok(hash)
    }

    /// A row or a file at `mine_to`.
    async fn mine_to_taken(
        &self,
        id: WorkspaceId,
        place: PlaceId,
        mine_to: &str,
    ) -> Result<bool, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT count(*) AS n FROM team_result_files WHERE workspace_id = ?1 AND path = ?2",
        )
        .bind(id.to_string())
        .bind(mine_to)
        .fetch_one(&self.pool)
        .await?
        .try_get::<i64, _>("n")?;
        if rows > 0 {
            return Ok(true);
        }
        let root = PathBuf::from(self.files.place(place).await.map_err(file_failed)?.path);
        let target = root.join(mine_to);
        tokio::task::spawn_blocking(move || target.exists())
            .await
            .map_err(|_| WorkspaceError::FilesFailed)
    }

    // ---- WS-TW-5 Waiting grants

    /// WS-TW-5: records an agent or access a colleague added. Nothing is granted: no `set_grant`, no
    /// `publish_grants`. A waiting grant for the same agent key is replaced.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-5
    pub async fn record_pending_grant(
        &self,
        g: NewPendingGrant,
    ) -> Result<PendingGrant, WorkspaceError> {
        self.not_in_bin(g.workspace_id).await?;
        check_pending_grant(&g)?;
        let damaged = |_| WorkspaceError::DatastoreUnavailable;
        let repository = serde_json::to_string(&g.repository).map_err(damaged)?;
        let access = serde_json::to_string(&g.access).map_err(damaged)?;
        let id = TeamGrantId::new();
        let ws = g.workspace_id;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE team_pending_grants SET state = 'replaced' \
             WHERE workspace_id = ?1 AND agent_key = ?2 AND state = 'waiting'",
        )
        .bind(ws.to_string())
        .bind(&g.agent_key)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO team_pending_grants (id, workspace_id, agent_key, agent_name, repository, agent_id, \
             agent_version, agent_instance_id, access, added_by, added_by_confirmed, added_at, state, decided_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'waiting', NULL)",
        )
        .bind(id.to_string())
        .bind(ws.to_string())
        .bind(&g.agent_key)
        .bind(&g.agent_name)
        .bind(repository)
        .bind(&g.agent_id)
        .bind(&g.agent_version)
        .bind(g.agent_instance_id.map(|a| a.to_string()))
        .bind(access)
        .bind(&g.added_by)
        .bind(i64::from(g.added_by_confirmed))
        .bind(g.added_at.to_wire())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.record(
            "team_grant_waiting",
            Some(ws),
            "A colleague added an agent or access that waits for this person's answer.",
            serde_json::json!({ "workspace_id": ws, "entries": g.access.len() }),
        )
        .await;
        log::workspace_event("team_grant_waiting", ws);
        self.changed(ws).await;
        Ok(PendingGrant {
            id,
            grant: g,
            state: PendingGrantState::Waiting,
            decided_at: None,
        })
    }

    /// WS-TW-5: the waiting grants of a workspace, newest first.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-5
    pub async fn pending_grants(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<PendingGrant>, WorkspaceError> {
        self.workspace(id).await?;
        let rows = sqlx::query(
            "SELECT * FROM team_pending_grants WHERE workspace_id = ?1 AND state = 'waiting' \
             ORDER BY added_at DESC, id DESC",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(grant_row).collect()
    }

    /// WS-TW-5: the person's answer to a waiting grant. The grants an `Allowed` answer makes are made
    /// by the caller with `set_grant` before it decides.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-5
    pub async fn decide_pending_grant(
        &self,
        grant: TeamGrantId,
        decision: GrantDecision,
    ) -> Result<PendingGrant, WorkspaceError> {
        let now = Timestamp::now();
        let state = match decision {
            GrantDecision::Allowed => PendingGrantState::Allowed,
            GrantDecision::Declined => PendingGrantState::Declined,
        };
        let done = sqlx::query(
            "UPDATE team_pending_grants SET state = ?2, decided_at = ?3 WHERE id = ?1 AND state = 'waiting'",
        )
        .bind(grant.to_string())
        .bind(to_text(&state))
        .bind(now.to_wire())
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Err(WorkspaceError::GrantNotFound);
        }
        let row = sqlx::query("SELECT * FROM team_pending_grants WHERE id = ?1")
            .bind(grant.to_string())
            .fetch_one(&self.pool)
            .await?;
        let decided = grant_row(&row)?;
        let ws = decided.grant.workspace_id;
        let decision_text = to_text(&decision);
        self.record(
            "team_grant_decided",
            Some(ws),
            "A waiting agent or access was answered.",
            serde_json::json!({ "workspace_id": ws, "decision": decision_text }),
        )
        .await;
        tracing::info!(
            component = crate::log::COMPONENT,
            workspace_id = ws.to_string().as_str(),
            decision = decision_text.as_str(),
            "team_grant_decided"
        );
        self.changed(ws).await;
        Ok(decided)
    }

    // ---- WS-TW-7 The end of a team workspace

    /// WS-TW-7: waiting grants are declined and the results folder becomes an ordinary folder of the
    /// workspace, with every file and every grant.
    // spec: ordin-desktop/components/WorkspaceService#WS-TW-7
    pub async fn end_team(&self, id: WorkspaceId) -> Result<(), WorkspaceError> {
        self.workspace(id).await?;
        let now = Timestamp::now().to_wire();
        let ws = id.to_string();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE team_pending_grants SET state = 'declined', decided_at = ?2 \
             WHERE workspace_id = ?1 AND state = 'waiting'",
        )
        .bind(&ws)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE team_results_folders SET ended_at = ?2 WHERE workspace_id = ?1 AND ended_at IS NULL",
        )
        .bind(&ws)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM team_result_files WHERE workspace_id = ?1")
            .bind(&ws)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        lock(&self.results_not_shared).retain(|(w, _)| *w != id);
        self.record(
            "team_workspace_ended",
            Some(id),
            "A team workspace became a workspace of this person alone.",
            serde_json::json!({ "workspace_id": id }),
        )
        .await;
        log::workspace_event("team_workspace_ended", id);
        self.changed(id).await;
        Ok(())
    }
}

/// Removes a results folder this call made and could not use (empty, so nothing is lost).
async fn remove_if_made(folder: PathBuf, created_here: bool) {
    if created_here {
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir(folder)).await;
    }
}

/// The set of (workspace, path) whose `result_path_not_shared` was logged.
pub(crate) type NotShared = HashSet<(WorkspaceId, String)>;
