//! The rows of `DesktopServiceApi` that the program composes or answers itself for the screens'
//! Phase 11 commands: `files.read_version` (ServiceApi.md SVC-SCR-1), `files.download`,
//! `files.save_copy` (OrdinDesktop.md DSK-REC-43), `diagnostics.support_export` (DSK-REC-41) and
//! `workspaces.tree` (DSK-REC-42).

use base64::Engine as _;
use ordin_service_api::commands::files::{FilesDownloadResult, FilesReadVersionResult, Preview};
use std::collections::HashMap;

use ordin_home_search::RunSummary;
use ordin_service_api::commands::{
    LogsCleared, RunningItem, SaveCopy, SavedCopy, SupportExport, WorkspaceTree,
};
use ordin_service_api::{command, CommandEnvelope, CommandName, CommandOutcome, CommandReply};
use ordin_service_api::{ServiceApiVersion, ServiceError};
use ordin_types::domain::runs::{CrewRunState, RunState};
use ordin_types::wire::ContentRef;
use ordin_types::{CodedError, CommandRequestId, CrewRunId, PlaceId, WorkspaceId};

use super::{DesktopServiceApi, INPUT_INVALID, INTERNAL};
use crate::error::{DIALOG_CANCELLED, SAVE_COPY_FAILED};

/// FileService.md Internal Logic 4.5: an image previewed up to 20 MiB, a PDF up to 64 MiB, and the
/// first 256 KiB of a text-like file.
const IMAGE_PREVIEW_LIMIT: usize = 20 * 1024 * 1024;
const PDF_PREVIEW_LIMIT: usize = 64 * 1024 * 1024;
const PREVIEW_TEXT_BYTES: usize = ordin_files::PREVIEW_TEXT_BYTES;
/// A `files.download` sent to a server waits at most this long (ServiceApi.md, budget `Transfer`).
const TRANSFER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// The lower-case extension of `path`'s last segment, without the dot.
fn kind_of(path: &str) -> String {
    let name = last_segment(path);
    match name.rfind('.') {
        Some(at) if at + 1 < name.len() => name[at + 1..].to_lowercase(),
        _ => String::new(),
    }
}

fn last_segment(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The text-like kinds of FileService.md Internal Logic 4.5 (the File Service's own list).
fn is_text_kind(kind: &str) -> bool {
    matches!(
        kind,
        "txt"
            | "md"
            | "markdown"
            | "csv"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "xml"
            | "log"
            | "rs"
            | "go"
            | "py"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "java"
            | "kt"
            | "swift"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "sh"
            | "sql"
            | "html"
            | "css"
            | "svelte"
    )
}

/// SVC-SCR-1: the preview of a version held only as bytes. An office kind has no thumbnail here,
/// because the operating system's thumbnail reads a file on disk.
// spec: ordin-desktop/components/ServiceApi#SVC-SCR-1
pub(crate) fn preview_of(path: &str, bytes: &[u8]) -> Preview {
    let kind = kind_of(path);
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    match kind.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => {
            if bytes.len() > IMAGE_PREVIEW_LIMIT {
                Preview::None
            } else {
                Preview::Image {
                    media_type: ordin_files::media_type(&kind).to_owned(),
                    bytes_b64: b64(bytes),
                }
            }
        }
        "pdf" if bytes.len() <= PDF_PREVIEW_LIMIT => Preview::Pdf {
            bytes_b64: b64(bytes),
        },
        k if is_text_kind(k) => {
            let head = &bytes[..bytes.len().min(PREVIEW_TEXT_BYTES)];
            Preview::Text {
                text: String::from_utf8_lossy(head).into_owned(),
                markdown: matches!(k, "md" | "markdown"),
                truncated: bytes.len() > PREVIEW_TEXT_BYTES,
            }
        }
        _ => Preview::None,
    }
}

/// The base64url form of `files.download`'s bytes (OrdinDesktop.md, the `files.save_copy` row).
fn encode_download(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_download(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .ok()
}

/// A later source of `workspaces.tree`: its value, or the empty value with `tree_source_failed`
/// logged (DSK-REC-42.2).
fn soft<T: Default, E: CodedError>(module: &'static str, result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| {
        tracing::warn!(
            component = "desktop",
            module,
            code = error.code(),
            "tree_source_failed"
        );
        T::default()
    })
}

/// DSK-REC-45.2 to 45.4: the summaries of one workspace as the tree's running items. `crew_states`
/// holds the state of each crew run the Crew Orchestrator could read.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-45
fn running_from(
    workspace_id: WorkspaceId,
    summaries: &[RunSummary],
    crew_states: &HashMap<CrewRunId, CrewRunState>,
) -> Vec<RunningItem> {
    let mut out = Vec::new();
    let mut crews_listed: Vec<CrewRunId> = Vec::new();
    for summary in summaries {
        if let Some(crew_run_id) = summary.crew_run_id {
            if let Some(state) = crew_states.get(&crew_run_id) {
                if !crews_listed.contains(&crew_run_id) {
                    crews_listed.push(crew_run_id);
                    out.push(RunningItem::CrewRun {
                        workspace_id,
                        crew_run_id,
                        title: summary.title.clone(),
                        state: *state,
                    });
                }
                continue;
            }
        }
        let typed =
            serde_json::from_value::<RunState>(serde_json::Value::String(summary.state.clone()));
        match (summary.run_id, typed) {
            (Some(run_id), Ok(state)) => out.push(RunningItem::Run {
                workspace_id,
                run_id,
                title: summary.title.clone(),
                state,
            }),
            _ => tracing::debug!(
                component = "desktop",
                workspace_id = workspace_id.to_string(),
                "tree_running_item_left_out"
            ),
        }
    }
    out
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl DesktopServiceApi {
    /// `files.read_version`: `FileService::read_version`, then the preview of its bytes; the bytes
    /// are dropped once the preview is built and never logged (DSK-REC-40.1).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-40
    pub(super) async fn files_read_version(
        &self,
        input: ordin_service_api::commands::files::FilesReadVersionInput,
    ) -> Result<FilesReadVersionResult, ServiceError> {
        let files = self.handle(&self.modules.files)?;
        let version = files
            .read_version(input.file_version_id)
            .await
            .map_err(|e| self.module_error(e))?;
        let preview = preview_of(&version.path, &version.bytes);
        Ok(FilesReadVersionResult {
            path: version.path,
            preview,
        })
    }

    /// `files.download` over `read_file`: the file's name, its type and its bytes in base64url.
    pub(super) async fn files_download(
        &self,
        place_id: PlaceId,
        path: &str,
    ) -> Result<FilesDownloadResult, ServiceError> {
        let files = self.handle(&self.modules.files)?;
        let read = files
            .read_file(place_id, path)
            .await
            .map_err(|e| self.module_error(e))?;
        Ok(FilesDownloadResult {
            name: last_segment(&read.path).to_owned(),
            media_type: read.media_type,
            bytes_b64: encode_download(&read.bytes),
        })
    }

    /// `files.download` on `backend`: this program's own, or the server's through the Server
    /// Connector, whose reply is read as the envelope it is.
    async fn download_on(
        &self,
        backend: ordin_types::BackendId,
        place_id: PlaceId,
        path: &str,
    ) -> Result<FilesDownloadResult, ServiceError> {
        if Some(backend) == self.modules.backend {
            return self.files_download(place_id, path).await;
        }
        let connector = self.handle(&self.modules.connector)?;
        let meta = command("files.download").ok_or_else(|| self.refuse(INTERNAL, None))?;
        let envelope = CommandEnvelope {
            api_version: ServiceApiVersion::CURRENT,
            request_id: CommandRequestId::new(),
            command: CommandName::new("files.download"),
            backend,
            input: serde_json::json!({ "place_id": place_id, "path": path }),
        };
        let body = serde_json::to_vec(&envelope).map_err(|_| self.refuse(INTERNAL, None))?;
        let reply = connector
            .send_command(
                backend,
                meta,
                body.into(),
                std::time::Instant::now() + TRANSFER_DEADLINE,
            )
            .await
            .map_err(|e| self.module_error(e))?;
        let reply: CommandReply<FilesDownloadResult> =
            serde_json::from_slice(&reply).map_err(|_| self.refuse(INTERNAL, None))?;
        match reply.outcome {
            CommandOutcome::Ok { result } => Ok(result),
            CommandOutcome::Error { error } => Err(error),
        }
    }

    /// The save dialog with `file_name` proposed, then an atomic write of `bytes`.
    async fn save_bytes(&self, file_name: &str, bytes: &[u8]) -> Result<SavedCopy, ServiceError> {
        let shell = self.handle(&self.modules.shell)?;
        let path = shell
            .save_path(file_name)
            .await
            .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
        crate::shell::write_atomically(&path, bytes)
            .map_err(|_| self.own_refusal(SAVE_COPY_FAILED, true))?;
        Ok(SavedCopy {
            saved_path: path.to_string_lossy().into_owned(),
        })
    }

    /// `files.save_copy` by the kind of item (DSK-REC-43): a file in a place, or an artefact that
    /// lies in a place; an exact version and an artefact in an agent folder are refused.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-43
    pub(super) async fn files_save_copy(&self, input: SaveCopy) -> Result<SavedCopy, ServiceError> {
        let refused = || self.refuse(INPUT_INVALID, None);
        let (place_id, path) = match input.item {
            ContentRef::PlaceFile {
                place_id,
                relative_path,
            } => (place_id, relative_path),
            ContentRef::Artefact { artefact_id } => {
                let artefact = self
                    .artefact_on(input.backend, artefact_id)
                    .await?
                    .ok_or_else(refused)?;
                match artefact.location {
                    ordin_types::domain::ArtefactLocation::Place { place_id } => {
                        (place_id, artefact.path)
                    }
                    _ => return Err(refused()),
                }
            }
            ContentRef::FileVersion { .. } => return Err(refused()),
        };
        // The dialog opens only once the item is known to be one the program saves.
        let download = self.download_on(input.backend, place_id, &path).await?;
        let bytes =
            decode_download(&download.bytes_b64).ok_or_else(|| self.refuse(INTERNAL, None))?;
        self.save_bytes(last_segment(&path), &bytes).await
    }

    /// `artefacts.get` on `backend` (ServiceApi.md SVC-SCR-3): the Journal's `artefact` here; a
    /// server's through the Server Connector.
    async fn artefact_on(
        &self,
        backend: ordin_types::BackendId,
        artefact_id: ordin_types::ArtefactId,
    ) -> Result<Option<ordin_types::domain::Artefact>, ServiceError> {
        if Some(backend) == self.modules.backend {
            let journal = self.handle(&self.modules.journal)?;
            return journal
                .artefact(artefact_id)
                .await
                .map_err(|e| self.module_error(e));
        }
        let connector = self.handle(&self.modules.connector)?;
        let meta = command("artefacts.get").ok_or_else(|| self.refuse(INTERNAL, None))?;
        let envelope = CommandEnvelope {
            api_version: ServiceApiVersion::CURRENT,
            request_id: CommandRequestId::new(),
            command: CommandName::new("artefacts.get"),
            backend,
            input: serde_json::json!({ "artefact_id": artefact_id }),
        };
        let body = serde_json::to_vec(&envelope).map_err(|_| self.refuse(INTERNAL, None))?;
        let reply = connector
            .send_command(
                backend,
                meta,
                body.into(),
                std::time::Instant::now() + TRANSFER_DEADLINE,
            )
            .await
            .map_err(|e| self.module_error(e))?;
        let reply: CommandReply<ordin_service_api::commands::artefacts::ArtefactsGetResult> =
            serde_json::from_slice(&reply).map_err(|_| self.refuse(INTERNAL, None))?;
        match reply.outcome {
            CommandOutcome::Ok { result } => Ok(result.artefact),
            CommandOutcome::Error { error } => Err(error),
        }
    }

    /// `diagnostics.support_export` (DSK-REC-41): the Journal's bundle and the last 3 days of the log
    /// files (DSK-LOG-5), then the save dialog and an atomic write. The bytes are never logged.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-41
    pub(super) async fn diagnostics_support_export(
        &self,
        input: SupportExport,
    ) -> Result<SavedCopy, ServiceError> {
        let journal = self.handle(&self.modules.journal)?;
        let mut bundle = journal
            .support_export(&input.run_ids)
            .await
            .map_err(|e| self.module_error(e))?;
        // DSK-LOG-5: the last 3 days of the log files, read through the process-wide handle.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-5
        let installed = crate::log_files::installed();
        let logs =
            tokio::task::spawn_blocking(move || crate::log_files::support_logs(installed.as_ref()))
                .await
                .map_err(|_| self.refuse(INTERNAL, None))?;
        bundle.bytes.extend_from_slice(&logs.bytes);
        let saved = self
            .save_bytes(&bundle.suggested_file_name, &bundle.bytes)
            .await?;
        tracing::info!(
            component = "desktop",
            truncated = bundle.truncated,
            log_files = logs.files,
            log_files_unreadable = logs.unreadable.len(),
            "support_export_saved"
        );
        Ok(saved)
    }

    /// `diagnostics.clear_logs` (DSK-LOG-4): the past log files deleted and the current one emptied,
    /// logging carrying on; any failure is `DSK_LOGS_CLEAR_FAILED` (retryable). Then the Diagnostics
    /// event list is emptied from now on (JA-DIAG-CLEAR-1): the journal keeps its entries and lists only
    /// those recorded after this moment; a journal refusal is its own code.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-4
    // spec: ordin-desktop/components/JournalAndArtefactStore#JA-DIAG-CLEAR-1
    pub(super) async fn diagnostics_clear_logs(&self) -> Result<LogsCleared, ServiceError> {
        let cleared = crate::log_files::clear_logs(crate::log_files::installed().as_ref())
            .map_err(|code| self.own_refusal(code, true))?;
        if let Some(journal) = self.modules.journal.as_ref() {
            journal
                .clear_diagnostic_lines()
                .await
                .map_err(|e| self.module_error(e))?;
        }
        Ok(cleared)
    }

    /// `workspaces.tree` (DSK-REC-42): one owner source per field; `workspace` and `places` first.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-42
    pub(super) async fn workspaces_tree(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<WorkspaceTree, ServiceError> {
        let workspaces = self.handle(&self.modules.workspaces)?;
        let workspace = workspaces
            .get(workspace_id)
            .await
            .map_err(|e| self.module_error(e))?;
        let places = workspaces
            .places(workspace_id)
            .await
            .map_err(|e| self.module_error(e))?;
        let (agents, workflows, dashboards, knowledge_pages, memory_facts, waiting) = tokio::join!(
            async {
                match &self.modules.packages {
                    Some(p) => soft(
                        "ordin-package-catalogue",
                        p.list(Some(workspace_id))
                            .await
                            .map(|v| v.into_iter().map(|a| a.instance).collect::<Vec<_>>()),
                    ),
                    None => Vec::new(),
                }
            },
            async {
                match &self.modules.ordin_host {
                    Some(h) => soft(
                        "ordin-host",
                        h.workflows(workspace_id)
                            .await
                            .map(|v| v.into_iter().map(|w| w.workflow_id).collect::<Vec<_>>()),
                    ),
                    None => Vec::new(),
                }
            },
            async {
                match &self.modules.journal {
                    Some(j) => soft("ordin-journal", self.dashboard_ids(j, workspace_id).await),
                    None => Vec::new(),
                }
            },
            async {
                match &self.modules.knowledge {
                    Some(k) => soft(
                        "ordin-knowledge",
                        k.list_pages(workspace_id, None)
                            .await
                            .map(|l| count(l.this_workspace.len())),
                    ),
                    None => 0,
                }
            },
            async {
                match &self.modules.memory {
                    Some(m) => soft(
                        "ordin-memory-service",
                        m.count_workspace_facts(workspace_id).await,
                    ),
                    None => 0,
                }
            },
            async {
                match &self.modules.home {
                    Some(h) => soft(
                        "ordin-home-search",
                        h.needs_you(Some(workspace_id))
                            .await
                            .map(|v| count(v.len())),
                    ),
                    None => 0,
                }
            },
        );
        let running = self.running_items(workspace_id).await;
        Ok(WorkspaceTree {
            workspace: self.restate(workspace)?,
            places: self.restate(places)?,
            agents: self.restate(agents)?,
            workflows,
            dashboards,
            running,
            knowledge_pages,
            memory_facts,
            waiting_for_person: waiting,
        })
    }

    /// The tree's running items (DSK-REC-45): Home's running list kept to the workspace, the runs as
    /// `Run` items and the rounds of one crew run as one `CrewRun` item.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-45
    async fn running_items(&self, workspace_id: WorkspaceId) -> Vec<RunningItem> {
        let Some(reader) = &self.modules.journal_reader else {
            return Vec::new();
        };
        let summaries = match reader.running().await {
            Ok(summaries) => summaries,
            Err(_) => {
                tracing::warn!(
                    component = "desktop",
                    module = "ordin-home-search",
                    code = "HOME_SOURCE_UNAVAILABLE",
                    "tree_source_failed"
                );
                return Vec::new();
            }
        };
        let summaries: Vec<RunSummary> = summaries
            .into_iter()
            .filter(|s| s.workspace_id == workspace_id)
            .collect();
        let mut crew_states: HashMap<CrewRunId, CrewRunState> = HashMap::new();
        if let Some(orchestrator) = &self.modules.orchestrator {
            for crew_run_id in summaries.iter().filter_map(|s| s.crew_run_id) {
                if let std::collections::hash_map::Entry::Vacant(slot) =
                    crew_states.entry(crew_run_id)
                {
                    if let Ok((crew, _)) = orchestrator.get(crew_run_id).await {
                        slot.insert(crew.state);
                    }
                }
            }
        }
        running_from(workspace_id, &summaries, &crew_states)
    }

    /// Every page of `list_dashboards`, from no cursor until `next_cursor` is null.
    async fn dashboard_ids(
        &self,
        journal: &ordin_journal::Journal,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<ordin_types::DashboardId>, ordin_journal::JournalError> {
        let mut ids = Vec::new();
        let mut cursor = None;
        loop {
            let page = journal.list_dashboards(workspace_id, cursor).await?;
            ids.extend(page.items.into_iter().map(|d| d.id));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(ids),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_version_is_previewed_as_markdown() {
        match preview_of("notes.md", b"# Plan") {
            Preview::Text {
                text,
                markdown,
                truncated,
            } => {
                assert_eq!(text, "# Plan");
                assert!(markdown);
                assert!(!truncated);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_long_text_version_is_cut() {
        let long = vec![b'a'; PREVIEW_TEXT_BYTES + 1];
        match preview_of("log.txt", &long) {
            Preview::Text {
                text,
                markdown,
                truncated,
            } => {
                assert_eq!(text.len(), PREVIEW_TEXT_BYTES);
                assert!(!markdown);
                assert!(truncated);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_office_version_has_no_thumbnail() {
        assert!(matches!(preview_of("contract.docx", b"PK"), Preview::None));
        assert!(matches!(preview_of("noext", b"x"), Preview::None));
    }

    #[test]
    fn images_and_pdfs_up_to_their_limits() {
        match preview_of("a/b/logo.PNG", b"\x89PNG") {
            Preview::Image { media_type, .. } => assert_eq!(media_type, "image/png"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(preview_of("r.pdf", b"%PDF"), Preview::Pdf { .. }));
        let big = vec![0u8; IMAGE_PREVIEW_LIMIT + 1];
        assert!(matches!(preview_of("big.jpg", &big), Preview::None));
    }

    #[test]
    fn download_bytes_round_trip_as_base64url() {
        let bytes = [0xfbu8, 0xff, 0x00, 0x10];
        let text = encode_download(&bytes);
        assert!(!text.contains('+') && !text.contains('/') && !text.contains('='));
        assert_eq!(decode_download(&text), Some(bytes.to_vec()));
        assert_eq!(decode_download("!!"), None);
    }

    #[test]
    fn names_and_kinds() {
        assert_eq!(last_segment("out/summary.md"), "summary.md");
        assert_eq!(kind_of("out/summary.MD"), "md");
        assert_eq!(kind_of("out/."), "");
    }
    fn summary(
        workspace_id: WorkspaceId,
        run: bool,
        crew: Option<CrewRunId>,
        title: &str,
        state: &str,
    ) -> RunSummary {
        RunSummary {
            workspace_id,
            run_id: run.then(ordin_types::RunId::new),
            crew_run_id: crew,
            title: title.to_owned(),
            state: state.to_owned(),
            at: ordin_types::time::Timestamp::now(),
            can_run_again: false,
        }
    }

    // Scenario: Happy path — the tree lists what is running in the workspace (DSK-REC-45)
    #[test]
    fn a_run_is_one_run_item() {
        let w = WorkspaceId::new();
        let items = running_from(
            w,
            &[summary(w, true, None, "Kestrel buyer", "running")],
            &HashMap::new(),
        );
        assert!(matches!(
            items.as_slice(),
            [RunningItem::Run { title, state: RunState::Running, .. }] if title == "Kestrel buyer"
        ));
    }

    // Scenario: Happy path — a crew run is one item (DSK-REC-45)
    #[test]
    fn the_rounds_of_a_crew_run_are_one_crew_run_item() {
        let w = WorkspaceId::new();
        let crew = CrewRunId::new();
        let summaries = [
            summary(w, true, Some(crew), "Buyer", "running"),
            summary(w, true, Some(crew), "Writer", "running"),
        ];
        let states = HashMap::from([(crew, CrewRunState::Running)]);
        let items = running_from(w, &summaries, &states);
        assert!(matches!(
            items.as_slice(),
            [RunningItem::CrewRun { title, state: CrewRunState::Running, crew_run_id, .. }]
                if title == "Buyer" && *crew_run_id == crew
        ));
    }

    // Scenario: Edge case — the crew run cannot be read, so its rounds are runs (DSK-REC-45.3)
    #[test]
    fn rounds_of_an_unreadable_crew_run_are_run_items() {
        let w = WorkspaceId::new();
        let crew = CrewRunId::new();
        let summaries = [
            summary(w, true, Some(crew), "Buyer", "running"),
            summary(w, true, Some(crew), "Writer", "paused"),
        ];
        let items = running_from(w, &summaries, &HashMap::new());
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| matches!(i, RunningItem::Run { .. })));
    }

    // Scenario: Edge case — a state the tree cannot type (DSK-REC-45)
    #[test]
    fn a_state_that_names_no_run_state_is_left_out() {
        let w = WorkspaceId::new();
        let items = running_from(
            w,
            &[
                summary(w, true, None, "Slack agent", "replied_in_slack"),
                summary(w, false, None, "No id", "running"),
            ],
            &HashMap::new(),
        );
        assert!(items.is_empty());
    }
}
