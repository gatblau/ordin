//! Folder grants: the stored working copy of each workspace's grant set and the grants applied to
//! each session (AgentFolderManager.md Internal Logic 6, 6a, 8, 9).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use ordin_types::domain::places::{FolderAccess, Grantee, PlaceKind};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, CrewCharterId, FolderGrantId, PlaceId, RunId, WorkspaceId};
use uuid::Uuid;

use crate::db::{failed, Arg, Tx};
use crate::error::AfmError;
use crate::manager::{guard, AgentFolderManager, LiveSession};
use crate::paths::{is_inside_any, valid_local_path};
use crate::types::{AppliedGrant, SessionAccess, WorkspaceGrant};

fn access_text(access: FolderAccess) -> &'static str {
    match access {
        FolderAccess::Read => "read",
        FolderAccess::ReadAndChange => "read_and_change",
    }
}

fn access_of(text: &str) -> Result<FolderAccess, AfmError> {
    match text {
        "read" => Ok(FolderAccess::Read),
        "read_and_change" => Ok(FolderAccess::ReadAndChange),
        _ => Err(AfmError::DatastoreFailed { retryable: false }),
    }
}

fn uuid_of(text: &str) -> Result<Uuid, AfmError> {
    Uuid::parse_str(text).map_err(|_| AfmError::DatastoreFailed { retryable: false })
}

/// `place_kind` as stored; `AgentFolder` is never stored (step 6a).
fn place_kind_text(kind: PlaceKind) -> Option<&'static str> {
    match kind {
        PlaceKind::PersonsFolder => Some("persons_folder"),
        PlaceKind::WorkspaceFolder => Some("workspace_folder"),
        PlaceKind::WorkingCopy => Some("working_copy"),
        PlaceKind::CodingTaskWorktree => Some("coding_task_worktree"),
        PlaceKind::AgentFolder => None,
    }
}

/// What step 6 finds when it looks at one granted place.
#[derive(Clone)]
enum Resolution {
    /// Missing or not a directory.
    Unreachable,
    /// Resolves (through links if need be) into ordin's data root.
    OrdinData,
    Reachable(PathBuf),
}

/// Looks each path up once per call: 200 grants across 20 sessions touch the disk 200 times.
struct Resolver<'a> {
    roots: [&'a Path; 2],
    cache: HashMap<PathBuf, Resolution>,
}

impl<'a> Resolver<'a> {
    fn new(roots: [&'a Path; 2]) -> Self {
        Resolver {
            roots,
            cache: HashMap::new(),
        }
    }

    fn resolve(&mut self, path: &Path) -> Resolution {
        if let Some(found) = self.cache.get(path) {
            return found.clone();
        }
        // The plain form, as the place roots are stored, so the comparison with them agrees.
        // spec: ordin-desktop/components/AgentFolderManager#AFM-PATH-1
        let canonical = std::fs::canonicalize(path).map(ordin_types::paths::plain_form);
        let resolution = match (std::fs::metadata(path), canonical) {
            (Ok(meta), Ok(canonical)) if meta.is_dir() => {
                if is_inside_any(&canonical, &self.roots) {
                    Resolution::OrdinData
                } else {
                    Resolution::Reachable(canonical)
                }
            }
            _ => Resolution::Unreachable,
        };
        self.cache.insert(path.to_path_buf(), resolution.clone());
        resolution
    }
}

/// One session whose rows are derived again.
struct SessionKey {
    run_id: RunId,
    live: LiveSession,
}

impl AgentFolderManager {
    /// Step 6a. Replaces the stored grant set of one workspace and re-derives the grants of every
    /// live session in that workspace. An empty `grants` clears the workspace.
    // spec: ordin-desktop/components/AgentFolderManager#SA-RECON-01
    pub async fn set_workspace_grants(
        &self,
        workspace_id: WorkspaceId,
        grants: Vec<WorkspaceGrant>,
    ) -> Result<(), AfmError> {
        if let Some(other) = grants.iter().find(|g| g.grant.workspace_id != workspace_id) {
            return Err(AfmError::GrantInvalid {
                folder_grant_id: other.grant.id,
            });
        }
        let _gate = self.write_gate.lock().await;
        // A skipped grant is left out and logged; the rest is still stored, so a withdrawal lands.
        let roots = self.ordin_roots();
        let mut kept = Vec::with_capacity(grants.len());
        let mut skipped = 0usize;
        for g in &grants {
            let usable = valid_local_path(&g.place_path)
                && !is_inside_any(&g.place_path, &roots)
                && place_kind_text(g.place_kind).is_some();
            if usable {
                kept.push(g);
            } else {
                skipped += 1;
                crate::log::grant_skipped_stored(g.grant.id, g.grant.place_id);
            }
        }
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_set_grants"))?;
        let workspace = Arg::text(workspace_id);
        tx.execute(
            "DELETE FROM workspace_folder_grants WHERE workspace_id = $1{uuid}",
            std::slice::from_ref(&workspace),
        )
        .await
        .map_err(failed("delete_workspace_grants"))?;
        for g in &kept {
            let (kind, grantee_id) = match g.grant.grantee {
                Grantee::AgentInstance { agent_instance_id } => {
                    ("agent_instance", agent_instance_id.to_string())
                }
                Grantee::Crew { crew_charter_id } => ("crew", crew_charter_id.to_string()),
            };
            let kind_text = place_kind_text(g.place_kind).unwrap_or("persons_folder");
            let path = g.place_path.to_str().unwrap_or_default();
            tx.execute(
                "INSERT INTO workspace_folder_grants (folder_grant_id, workspace_id, grantee_kind, \
                 grantee_id, place_id, place_kind, path, access, granted_at) \
                 VALUES ($1{uuid}, $2{uuid}, $3, $4{uuid}, $5{uuid}, $6, $7, $8, $9{ts})",
                &[
                    Arg::text(g.grant.id),
                    workspace.clone(),
                    Arg::text(kind),
                    Arg::text(grantee_id),
                    Arg::text(g.grant.place_id),
                    Arg::text(kind_text),
                    Arg::text(path),
                    Arg::text(access_text(g.grant.access)),
                    Arg::text(g.grant.granted_at.to_wire()),
                ],
            )
            .await
            .map_err(failed("insert_workspace_grant"))?;
        }
        // Every live session of the workspace: rows already held, plus sessions opened with no grant yet.
        let sessions = self.live_sessions_of(&mut tx, workspace_id).await?;
        let mut resolver = Resolver::new(roots);
        for session in &sessions {
            derive_session(&mut tx, &mut resolver, session.run_id, &session.live).await?;
        }
        tx.commit().await.map_err(failed("commit_set_grants"))?;
        crate::log::workspace_grants_set(workspace_id, kept.len(), skipped, sessions.len());
        self.record(
            "workspace_folder_grants_set",
            None,
            Some(workspace_id),
            "Workspace folder grants replaced.",
            serde_json::json!({
                "workspace_id": workspace_id,
                "grants": kept.len(),
                "skipped": skipped,
                "sessions_updated": sessions.len(),
            }),
        )
        .await;
        Ok(())
    }

    /// Step 6. Session start and resume: derives the session's grants from the stored set of
    /// `workspace_id` — every grant whose grantee is the instance, or the crew charter when
    /// `crew_charter_id` is set. Calling it again for the same run repeats the derivation.
    pub async fn apply_grants(
        &self,
        run_id: RunId,
        id: AgentInstanceId,
        workspace_id: WorkspaceId,
        crew_charter_id: Option<CrewCharterId>,
    ) -> Result<SessionAccess, AfmError> {
        let _gate = self.write_gate.lock().await;
        let row = self.load_created(id).await?;
        let live = LiveSession {
            agent_instance_id: id,
            workspace_id,
            crew_charter_id,
        };
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_apply_grants"))?;
        let mut resolver = Resolver::new(self.ordin_roots());
        let grants = derive_session(&mut tx, &mut resolver, run_id, &live).await?;
        tx.commit().await.map_err(failed("commit_apply_grants"))?;
        guard(&self.live).insert(run_id, live);
        crate::log::grants_applied(run_id, id, workspace_id, grants.len());
        let summary: Vec<serde_json::Value> = grants
            .iter()
            .map(|g| serde_json::json!({ "place_id": g.place_id, "access": g.access }))
            .collect();
        self.record(
            "folder_grants_applied",
            Some(run_id),
            Some(workspace_id),
            "Folder grants applied to a session.",
            serde_json::json!({
                "run_id": run_id,
                "agent_instance_id": id,
                "workspace_id": workspace_id,
                "grants": summary,
            }),
        )
        .await;
        Ok(SessionAccess {
            run_id,
            agent_instance_id: id,
            workspace_id,
            agent_folder: row.folder_path,
            grants,
        })
    }

    /// The grants now held by a session; a grant withdrawn since (step 8) is no longer listed.
    pub async fn session_access(&self, run_id: RunId) -> Result<SessionAccess, AfmError> {
        let unknown = AfmError::SessionUnknown { run_id };
        let rows = crate::db::fetch_all(
            &self.store,
            "SELECT {s:place_id}, path, access, {s:agent_instance_id}, {s:workspace_id} \
             FROM session_folder_access WHERE run_id = $1{uuid} ORDER BY path, place_id",
            &[Arg::text(run_id)],
        )
        .await
        .map_err(failed("select_session_access"))?;
        let remembered = guard(&self.live).get(&run_id).copied();
        let (id, workspace_id) = match (remembered, rows.first()) {
            (Some(live), _) => (live.agent_instance_id, live.workspace_id),
            (None, Some(row)) => (
                AgentInstanceId(uuid_of(
                    &row.text(3).map_err(failed("read_session_access"))?,
                )?),
                WorkspaceId(uuid_of(
                    &row.text(4).map_err(failed("read_session_access"))?,
                )?),
            ),
            (None, None) => return Err(unknown),
        };
        let mut grants = Vec::with_capacity(rows.len());
        for row in &rows {
            grants.push(AppliedGrant {
                place_id: PlaceId(uuid_of(
                    &row.text(0).map_err(failed("read_session_access"))?,
                )?),
                path: PathBuf::from(row.text(1).map_err(failed("read_session_access"))?),
                access: access_of(&row.text(2).map_err(failed("read_session_access"))?)?,
            });
        }
        let folder = self.load_created(id).await.map_err(|_| unknown)?;
        Ok(SessionAccess {
            run_id,
            agent_instance_id: id,
            workspace_id,
            agent_folder: folder.folder_path,
            grants,
        })
    }

    /// Step 9. Deletes the run's rows and drops the run from the live-session list; an unknown run
    /// is `Ok(())`.
    pub async fn end_session(&self, run_id: RunId) -> Result<(), AfmError> {
        let _gate = self.write_gate.lock().await;
        let mut tx = Tx::begin(&self.store)
            .await
            .map_err(failed("begin_end_session"))?;
        tx.execute(
            "DELETE FROM session_folder_access WHERE run_id = $1{uuid}",
            &[Arg::text(run_id)],
        )
        .await
        .map_err(failed("delete_session_access"))?;
        tx.commit().await.map_err(failed("commit_end_session"))?;
        guard(&self.live).remove(&run_id);
        Ok(())
    }

    /// The live sessions of one workspace: distinct runs in `session_folder_access`, plus each run
    /// the in-memory list holds for it.
    async fn live_sessions_of(
        &self,
        tx: &mut Tx,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<SessionKey>, AfmError> {
        let mut sessions: BTreeMap<RunId, LiveSession> = guard(&self.live)
            .iter()
            .filter(|(_, live)| live.workspace_id == workspace_id)
            .map(|(run, live)| (*run, *live))
            .collect();
        let rows = tx
            .fetch_all(
                "SELECT DISTINCT {s:run_id}, {s:agent_instance_id} FROM session_folder_access \
                 WHERE workspace_id = $1{uuid}",
                &[Arg::text(workspace_id)],
            )
            .await
            .map_err(failed("select_live_sessions"))?;
        for row in &rows {
            let run = RunId(uuid_of(
                &row.text(0).map_err(failed("read_live_sessions"))?,
            )?);
            let instance = AgentInstanceId(uuid_of(
                &row.text(1).map_err(failed("read_live_sessions"))?,
            )?);
            sessions.entry(run).or_insert(LiveSession {
                agent_instance_id: instance,
                workspace_id,
                crew_charter_id: None,
            });
        }
        Ok(sessions
            .into_iter()
            .map(|(run_id, live)| SessionKey { run_id, live })
            .collect())
    }
}

/// Step 6's derivation: reads the stored grants for the session's grantees, checks each place,
/// and replaces the run's rows in the open transaction. Returns what was applied.
async fn derive_session(
    tx: &mut Tx,
    resolver: &mut Resolver<'_>,
    run_id: RunId,
    live: &LiveSession,
) -> Result<Vec<AppliedGrant>, AfmError> {
    let mut args = vec![
        Arg::text(live.workspace_id),
        Arg::text(live.agent_instance_id),
    ];
    let mut grantees = "(grantee_kind = 'agent_instance' AND grantee_id = $2{uuid})".to_owned();
    if let Some(charter) = live.crew_charter_id {
        args.push(Arg::text(charter));
        grantees.push_str(" OR (grantee_kind = 'crew' AND grantee_id = $3{uuid})");
    }
    let stored = tx
        .fetch_all(
            &format!(
                "SELECT {{s:folder_grant_id}}, {{s:place_id}}, path, access FROM workspace_folder_grants \
                 WHERE workspace_id = $1{{uuid}} AND ({grantees}) ORDER BY granted_at, folder_grant_id"
            ),
            &args,
        )
        .await
        .map_err(failed("select_workspace_grants"))?;
    // A place granted to both the instance and its crew charter would break the (run_id, place_id)
    // key. The broader access wins (`read_and_change` over `read`); the first
    // grant of the place by `granted_at` is the one recorded.
    let mut by_place: BTreeMap<PlaceId, (FolderGrantId, AppliedGrant)> = BTreeMap::new();
    for row in &stored {
        let grant_id = FolderGrantId(uuid_of(
            &row.text(0).map_err(failed("read_workspace_grant"))?,
        )?);
        let place_id = PlaceId(uuid_of(
            &row.text(1).map_err(failed("read_workspace_grant"))?,
        )?);
        let path = PathBuf::from(row.text(2).map_err(failed("read_workspace_grant"))?);
        let access = access_of(&row.text(3).map_err(failed("read_workspace_grant"))?)?;
        match resolver.resolve(&path) {
            Resolution::Unreachable => crate::log::grant_unreachable(place_id),
            Resolution::OrdinData => crate::log::grant_skipped_session(place_id),
            Resolution::Reachable(canonical) => {
                let applied = AppliedGrant {
                    place_id,
                    path: canonical,
                    access,
                };
                match by_place.get_mut(&place_id) {
                    Some((_, held)) => {
                        if access == FolderAccess::ReadAndChange {
                            held.access = access;
                        }
                    }
                    None => {
                        by_place.insert(place_id, (grant_id, applied));
                    }
                }
            }
        }
    }
    let mut applied: Vec<(FolderGrantId, AppliedGrant)> = by_place.into_values().collect();
    applied.sort_by(|a, b| (&a.1.path, a.1.place_id).cmp(&(&b.1.path, b.1.place_id)));
    tx.execute(
        "DELETE FROM session_folder_access WHERE run_id = $1{uuid}",
        &[Arg::text(run_id)],
    )
    .await
    .map_err(failed("delete_session_access"))?;
    let applied_at = Timestamp::now().to_wire();
    for (grant_id, grant) in &applied {
        tx.execute(
            "INSERT INTO session_folder_access (run_id, agent_instance_id, workspace_id, \
             folder_grant_id, place_id, path, access, applied_at) \
             VALUES ($1{uuid}, $2{uuid}, $3{uuid}, $4{uuid}, $5{uuid}, $6, $7, $8{ts})",
            &[
                Arg::text(run_id),
                Arg::text(live.agent_instance_id),
                Arg::text(live.workspace_id),
                Arg::text(grant_id),
                Arg::text(grant.place_id),
                Arg::text(grant.path.to_str().unwrap_or_default()),
                Arg::text(access_text(grant.access)),
                Arg::text(&applied_at),
            ],
        )
        .await
        .map_err(failed("insert_session_access"))?;
    }
    Ok(applied.into_iter().map(|(_, grant)| grant).collect())
}
