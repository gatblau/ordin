//! Who wrote a file (FileService.md Internal Logic 6a).
//!
//! A harness's own file tools write straight to the disk, so the watcher alone would count their
//! writes as the person's. The Hook Relay therefore opens a claim before such a call runs and settles
//! it after: a change under an open claim is the run's. A call that cannot open its claim does not
//! run, so an agent's write never reaches the disk unclaimed. A tool whose writes cannot be told from
//! its arguments (a shell, a package's server) holds one wide claim until its run ends, over every
//! place, because neither is confined to the places granted.
// spec: ordin-desktop/components/FileService#SA-RECON-14

use std::collections::BTreeSet;
use std::path::PathBuf;

use ordin_types::domain::places::FileVersionOrigin;
use ordin_types::time::Timestamp;
use ordin_types::{PlaceId, RunId};
use sqlx::Row;

use crate::error::FileError;
use crate::paths;
use crate::service::FileService;
use crate::versions::blocking;

/// `FileVersionOrigin::Run.tool` holds at most this many characters.
const MAX_TOOL_CHARS: usize = 64;

/// What a tool call may write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentWriteTarget {
    /// One absolute path.
    File(PathBuf),
    /// Anything: the call's writes cannot be told from its arguments.
    Anywhere,
}

/// The claim over a path.
pub(crate) struct Claim {
    pub(crate) run_id: RunId,
    pub(crate) tool: String,
    pub(crate) wide: bool,
}

impl Claim {
    pub(crate) fn origin(&self) -> FileVersionOrigin {
        FileVersionOrigin::Run {
            run_id: self.run_id,
            tool: self.tool.clone(),
        }
    }
}

/// One claim row with no path: a whole place, or with no place, every place.
pub(crate) struct WideRow {
    id: String,
    run_id: String,
    tool: String,
    place: Option<PlaceId>,
    opened_at: String,
}

/// Claims left by the last process, gathered before anything new can open.
#[derive(Default)]
pub(crate) struct LeftOver {
    files: Vec<String>,
    wide: Vec<WideRow>,
}

/// macOS and Windows find a file under any case, so a claim does too.
fn folded(path: &str) -> String {
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        path.to_lowercase()
    } else {
        path.to_owned()
    }
}

fn parsed_run(raw: &str) -> Result<RunId, FileError> {
    RunId::parse(raw).map_err(|_| FileError::DatastoreUnavailable)
}

impl FileService {
    /// Opens a claim before a tool call runs. `Ok(None)` when nothing needs settling after the call:
    /// a path in no place, or a wide claim, which lasts until `agent_run_ended`. A path whose target
    /// cannot be known takes a wide claim. An error means the write could not be attributed, so the
    /// call must not run.
    pub async fn open_agent_write(
        &self,
        run_id: RunId,
        tool: &str,
        target: AgentWriteTarget,
    ) -> Result<Option<String>, FileError> {
        let tool: String = tool.chars().take(MAX_TOOL_CHARS).collect();
        let path = match target {
            AgentWriteTarget::Anywhere => None,
            AgentWriteTarget::File(path) => {
                blocking(move || paths::canonicalise_strictly(&path)).await?
            }
        };
        let Some(path) = path else {
            self.open_wide(run_id, &tool).await?;
            return Ok(None);
        };
        // Places can nest, and each one's watcher sees the file.
        let held: Vec<(PlaceId, String)> = self
            .all_place_rows()
            .await?
            .iter()
            .filter_map(|row| Some((row.id, paths::relative_to(&row.root, &path)?)))
            .filter(|(_, rel)| !rel.is_empty())
            .collect();
        if held.is_empty() {
            return Ok(None);
        }
        let claim = uuid::Uuid::now_v7().to_string();
        let now = Timestamp::now().to_wire();
        let mut tx = self.begin_write().await?;
        for (place_id, rel) in held {
            sqlx::query(
                "INSERT INTO agent_write_claims \
                     (id, claim, run_id, tool, place_id, path, opened_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&claim)
            .bind(run_id.to_string())
            .bind(&tool)
            .bind(place_id.to_string())
            .bind(rel)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(Some(claim))
    }

    async fn open_wide(&self, run_id: RunId, tool: &str) -> Result<(), FileError> {
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO agent_write_claims (id, claim, run_id, tool, opened_at) \
             VALUES (?1, ?1, ?2, ?3, ?4) \
             ON CONFLICT (run_id) WHERE place_id IS NULL \
             DO UPDATE SET opened_at = excluded.opened_at",
        )
        .bind(id)
        .bind(run_id.to_string())
        .bind(tool)
        .bind(Timestamp::now().to_wire())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The call has ended: its write, if any, becomes a version of the run in every place holding
    /// the path, and the claim closes. On an error the claim stays, and the path stays unattributed
    /// until the run ends.
    pub async fn settle_agent_write(&self, claim: &str) -> Result<(), FileError> {
        let rows = sqlx::query(
            "SELECT place_id, path FROM agent_write_claims \
             WHERE claim = ?1 AND path IS NOT NULL",
        )
        .bind(claim)
        .fetch_all(&self.pool)
        .await?;
        for row in &rows {
            let place: String = row.try_get("place_id")?;
            let path: String = row.try_get("path")?;
            let place = PlaceId::parse(&place).map_err(|_| FileError::DatastoreUnavailable)?;
            self.attribute(place, &path).await?;
        }
        sqlx::query("DELETE FROM agent_write_claims WHERE claim = ?1 AND path IS NOT NULL")
            .bind(claim)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records what is on the disk at a claimed path, as the claim's run wrote it.
    async fn attribute(&self, place_id: PlaceId, rel: &str) -> Result<(), FileError> {
        let place = match self.place_row(place_id).await {
            Ok(place) => place,
            Err(FileError::PlaceNotFound) => return Ok(()),
            Err(other) => return Err(other),
        };
        let abs = paths::confine(&place.root, rel)?;
        let present = blocking({
            let abs = abs.clone();
            move || abs.is_file()
        })
        .await?;
        if !present {
            return Ok(());
        }
        let is_new = self.index_row_missing(place_id, rel).await?;
        let change = self.index_changed_file(&place, rel, &abs, is_new).await?;
        if let Some(change) = change.filter(|change| change.version.is_some()) {
            self.emit(change);
        }
        Ok(())
    }

    /// The run's session has ended: every path claim settles, and each wide claim closes for the
    /// places a rescan really covered. On an error the claims stay until the next start.
    pub async fn agent_run_ended(&self, run_id: RunId) -> Result<(), FileError> {
        let (files, wide) = self.claims_of(Some(run_id)).await?;
        for claim in &files {
            self.settle_agent_write(claim).await?;
        }
        self.close_wide(wide).await
    }

    /// Closes wide claims. A place no rescan covered (its folder is missing, or the service is
    /// stopping) keeps a claim of its own, so its changes go to a person, until a start can cover
    /// it. Nothing the watcher did not reach becomes the person's.
    async fn close_wide(&self, wide: Vec<WideRow>) -> Result<(), FileError> {
        if wide.is_empty() {
            return Ok(());
        }
        let everywhere = wide.iter().any(|row| row.place.is_none());
        let places: BTreeSet<PlaceId> = if everywhere {
            let rows = self.all_place_rows().await?;
            rows.iter().map(|row| row.id).collect()
        } else {
            wide.iter().filter_map(|row| row.place).collect()
        };
        let mut uncovered = BTreeSet::new();
        for place in places {
            match self.rescan_covering(place).await {
                Ok(true) | Err(FileError::PlaceNotFound) => {}
                Ok(false) => {
                    uncovered.insert(place);
                }
                Err(other) => return Err(other),
            }
        }
        for row in wide {
            let mut tx = self.begin_write().await?;
            // A resumed session may have used the claim during the rescan: then it stays.
            let closed =
                sqlx::query("DELETE FROM agent_write_claims WHERE id = ?1 AND opened_at = ?2")
                    .bind(&row.id)
                    .bind(&row.opened_at)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
            if closed == 1 {
                for place in uncovered
                    .iter()
                    .filter(|place| row.place.is_none_or(|own| own == **place))
                {
                    let id = uuid::Uuid::now_v7().to_string();
                    sqlx::query(
                        "INSERT INTO agent_write_claims \
                             (id, claim, run_id, tool, place_id, opened_at) \
                         VALUES (?1, ?1, ?2, ?3, ?4, ?5) ON CONFLICT DO NOTHING",
                    )
                    .bind(id)
                    .bind(&row.run_id)
                    .bind(&row.tool)
                    .bind(place.to_string())
                    .bind(&row.opened_at)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            tx.commit().await?;
        }
        Ok(())
    }

    /// The claims of one run, or of every run: the path claims' names and the wide rows.
    async fn claims_of(
        &self,
        run_id: Option<RunId>,
    ) -> Result<(Vec<String>, Vec<WideRow>), FileError> {
        let rows = sqlx::query(
            "SELECT id, claim, run_id, tool, place_id, path, opened_at FROM agent_write_claims \
             WHERE ?1 IS NULL OR run_id = ?1",
        )
        .bind(run_id.map(|run| run.to_string()))
        .fetch_all(&self.pool)
        .await?;
        let mut files = BTreeSet::new();
        let mut wide = Vec::new();
        for row in &rows {
            let path: Option<String> = row.try_get("path")?;
            if path.is_some() {
                files.insert(row.try_get::<String, _>("claim")?);
                continue;
            }
            let place: Option<String> = row.try_get("place_id")?;
            let place = place
                .map(|raw| PlaceId::parse(&raw).map_err(|_| FileError::DatastoreUnavailable))
                .transpose()?;
            wide.push(WideRow {
                id: row.try_get("id")?,
                run_id: row.try_get("run_id")?,
                tool: row.try_get("tool")?,
                place,
                opened_at: row.try_get("opened_at")?,
            });
        }
        Ok((files.into_iter().collect(), wide))
    }

    /// Whether a claim is open over a path: a change there may be an agent's, whatever the versions
    /// say yet.
    pub async fn agent_write_open(&self, place_id: PlaceId, path: &str) -> Result<bool, FileError> {
        Ok(self.claim_over(place_id, path).await?.is_some())
    }

    /// The claim covering a path; a path claim wins over a wide one.
    pub(crate) async fn claim_over(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<Option<Claim>, FileError> {
        let rows = sqlx::query(
            "SELECT run_id, tool, path FROM agent_write_claims \
             WHERE place_id = ?1 OR place_id IS NULL ORDER BY path IS NULL, opened_at",
        )
        .bind(place_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let wanted = folded(path);
        for row in &rows {
            let claimed: Option<String> = row.try_get("path")?;
            if claimed.as_deref().is_none_or(|c| folded(c) == wanted) {
                let run_id: String = row.try_get("run_id")?;
                return Ok(Some(Claim {
                    run_id: parsed_run(&run_id)?,
                    tool: row.try_get("tool")?,
                    wide: claimed.is_none(),
                }));
            }
        }
        Ok(None)
    }

    async fn index_row_missing(&self, place_id: PlaceId, rel: &str) -> Result<bool, FileError> {
        let row = sqlx::query("SELECT 1 FROM file_index WHERE place_id = ?1 AND path = ?2")
            .bind(place_id.to_string())
            .bind(rel)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.is_none())
    }

    pub(crate) async fn left_over_claims(&self) -> Result<LeftOver, FileError> {
        let (files, wide) = self.claims_of(None).await?;
        Ok(LeftOver { files, wide })
    }

    /// The path claims of a process that ended without settling them.
    pub(crate) async fn settle_left_over(&self, left: &LeftOver) -> Result<(), FileError> {
        for claim in &left.files {
            self.settle_agent_write(claim).await?;
        }
        Ok(())
    }

    /// The wide claims of that process.
    pub(crate) async fn close_left_over(&self, left: LeftOver) -> Result<(), FileError> {
        self.close_wide(left.wide).await
    }
}
