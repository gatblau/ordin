//! Ordin's minutes and transcription tools (OrdinHost.md OH-MIN-4 to OH-MIN-11), built only with
//! the feature `minutes`. The host reaches the Minutes Service through [`MinutesPort`], which the
//! desktop program implements (OrdinDesktop.md DSK-MIN-6): this crate is built into more than one
//! program and may not depend on the desktop-only `ordin-minutes`.
//!
//! Recording, the two consents, the OBS password, the minutes settings, removing the speech model,
//! deleting a recording and removing minutes have no tool here (OH-MIN-9).

use ordin_mcp_front::{ArgumentProblem, ToolFailure, ToolOutput};
use ordin_types::id::{MinutesId, MinutesJobId, PlaceId, WorkspaceId};
use ordin_types::paging::{Cursor, Page};
use ordin_types::wire::minutes::{
    Minutes, MinutesJob, MinutesJobPurpose, MinutesJobState, MinutesSource, MinutesSummary,
    Transcript,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ports::DepError;
use crate::tools::{fail, len, out, typed};
use crate::OrdinHost;

/// The eleven tools, in the order of the tool table (OH-MIN-7).
pub(crate) const MINUTES_TOOLS: &[&str] = &[
    "list_minutes",
    "read_minutes",
    "read_minutes_transcript",
    "edit_minutes",
    "list_minutes_jobs",
    "cancel_minutes_job",
    "retry_minutes_job",
    "import_minutes",
    "export_minutes",
    "transcribe_recording",
    "read_transcription",
];

/// A file in one of the workspace's folders on this computer (OH-MIN-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    pub place_id: PlaceId,
    /// Relative to the folder, as Ordin gave it, checked by [`check_path`].
    pub path: String,
}

/// The two file formats of `export_minutes` (OH-MIN-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MinutesExportFormat {
    Pdf,
    Docx,
}

impl MinutesExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            MinutesExportFormat::Pdf => "pdf",
            MinutesExportFormat::Docx => "docx",
        }
    }
}

/// How the host reaches the Minutes Service; implemented by the desktop program (DSK-MIN-6). An
/// error keeps its owner's code and sentence (`MIN_*`, `TRX_*`, and the `OH_MINUTES_*` codes the
/// program answers for a folder or path it refuses).
// spec: ordin-desktop/components/OrdinHost#OH-MIN-5
#[async_trait::async_trait]
pub trait MinutesPort: Send + Sync {
    async fn list(
        &self,
        ws: WorkspaceId,
        cursor: Option<Cursor>,
    ) -> Result<Page<MinutesSummary>, DepError>;
    async fn get(&self, id: MinutesId) -> Result<Minutes, DepError>;
    async fn transcript(&self, id: MinutesId) -> Result<Transcript, DepError>;
    async fn save_edit(
        &self,
        id: MinutesId,
        body: String,
        base_revision: u32,
    ) -> Result<Minutes, DepError>;
    async fn jobs(&self, ws: WorkspaceId) -> Result<Vec<MinutesJob>, DepError>;
    async fn cancel_job(&self, job: MinutesJobId) -> Result<MinutesJob, DepError>;
    async fn retry(&self, job: MinutesJobId) -> Result<MinutesJob, DepError>;
    /// MinutesService `import` with the file's absolute path (OH-MIN-6).
    async fn import(&self, ws: WorkspaceId, file: LocalFile) -> Result<MinutesJob, DepError>;
    /// MinutesService `transcribe` (MinutesService.md MIN-TR-1).
    async fn transcribe(&self, ws: WorkspaceId, file: LocalFile) -> Result<MinutesJob, DepError>;
    /// MinutesService `transcription` (MIN-TR-3).
    async fn transcription(&self, job: MinutesJobId) -> Result<Transcript, DepError>;
    /// Writes the minutes as PDF or Word to `target` (OH-MIN-8 naming); returns the path written,
    /// relative to the folder.
    async fn export(
        &self,
        id: MinutesId,
        format: MinutesExportFormat,
        target: LocalFile,
    ) -> Result<String, DepError>;
}

// ------------------------------------------------------------------------- argument types

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListMinutesArgs {
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadMinutesArgs {
    pub minutes_id: MinutesId,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditMinutesArgs {
    pub minutes_id: MinutesId,
    pub body: String,
    pub base_revision: u32,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListMinutesJobsArgs {}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MinutesJobArgs {
    pub job_id: MinutesJobId,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MinutesFileArgs {
    /// The name of one of the workspace's folders on this computer.
    pub folder: String,
    /// Relative to the folder.
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExportMinutesArgs {
    pub minutes_id: MinutesId,
    pub format: MinutesExportFormat,
    pub folder: String,
    #[serde(default)]
    pub path: Option<String>,
}

/// The table rows of the eleven tools (OH-MIN-7): name, description, argument form, plain words
/// and schema.
pub(crate) fn specs() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Value,
)> {
    use crate::tools::schema;
    vec![
        (
            "list_minutes",
            "List this workspace's meeting minutes, newest first.",
            "list_minutes(cursor?: text)",
            "Can find your meeting minutes.",
            schema::<ListMinutesArgs>(),
        ),
        (
            "read_minutes",
            "Read one set of minutes with its revision.",
            "read_minutes(minutes_id)",
            "Can read your meeting minutes.",
            schema::<ReadMinutesArgs>(),
        ),
        (
            "read_minutes_transcript",
            "Read the transcript the minutes were written from.",
            "read_minutes_transcript(minutes_id)",
            "Can read what was said in a meeting.",
            schema::<ReadMinutesArgs>(),
        ),
        (
            "edit_minutes",
            "Replace the minutes' text; pass the revision you read.",
            "edit_minutes(minutes_id, body: markdown, base_revision)",
            "Can change meeting minutes when you ask.",
            schema::<EditMinutesArgs>(),
        ),
        (
            "list_minutes_jobs",
            "List the meetings being turned into minutes or text, with their state.",
            "list_minutes_jobs()",
            "Can see which meetings are being worked on.",
            schema::<ListMinutesJobsArgs>(),
        ),
        (
            "cancel_minutes_job",
            "Stop work on one meeting.",
            "cancel_minutes_job(job_id)",
            "Can stop work on a meeting.",
            schema::<MinutesJobArgs>(),
        ),
        (
            "retry_minutes_job",
            "Try a failed meeting again.",
            "retry_minutes_job(job_id)",
            "Can try a meeting again.",
            schema::<MinutesJobArgs>(),
        ),
        (
            "import_minutes",
            "Make minutes from a transcript or recording in a workspace folder.",
            "import_minutes(folder: \"<link name>\", path)",
            "Can make minutes from a file you name.",
            schema::<MinutesFileArgs>(),
        ),
        (
            "export_minutes",
            "Save minutes as PDF or Word in a workspace folder.",
            "export_minutes(minutes_id, format: \"pdf\"|\"docx\", folder: \"<link name>\", path?)",
            "Can save minutes as PDF or Word.",
            schema::<ExportMinutesArgs>(),
        ),
        (
            "transcribe_recording",
            "Turn a recording in a workspace folder into text, on this computer.",
            "transcribe_recording(folder: \"<link name>\", path)",
            "Can turn a recording into text on this computer.",
            schema::<MinutesFileArgs>(),
        ),
        (
            "read_transcription",
            "Read the text of a recording that transcribe_recording finished.",
            "read_transcription(job_id)",
            "Can read a recording it turned into text.",
            schema::<MinutesJobArgs>(),
        ),
    ]
}

/// The argument checks of the eleven tools; `None` for any other tool (OH-MIN-7).
pub(crate) fn check_arguments(tool: &str, a: &Value) -> Option<Result<(), ArgumentProblem>> {
    let result = match tool {
        "list_minutes" => typed::<ListMinutesArgs>(a).and_then(|l| match &l.cursor {
            Some(c) => len("/cursor", c, 0, 512),
            None => Ok(()),
        }),
        "read_minutes" | "read_minutes_transcript" => typed::<ReadMinutesArgs>(a).map(|_| ()),
        "edit_minutes" => typed::<EditMinutesArgs>(a).and_then(|e| {
            if e.body.is_empty() || e.body.len() > crate::minutes_tools::MAX_BODY_BYTES {
                return Err(ArgumentProblem {
                    at: "/body".into(),
                    reason: "1 byte to 1 MiB".into(),
                });
            }
            if e.base_revision < 1 {
                return Err(ArgumentProblem {
                    at: "/base_revision".into(),
                    reason: "at least 1".into(),
                });
            }
            Ok(())
        }),
        "list_minutes_jobs" => typed::<ListMinutesJobsArgs>(a).map(|_| ()),
        "cancel_minutes_job" | "retry_minutes_job" | "read_transcription" => {
            typed::<MinutesJobArgs>(a).map(|_| ())
        }
        "import_minutes" | "transcribe_recording" => typed::<MinutesFileArgs>(a).and_then(|f| {
            len("/folder", &f.folder, 1, 200)?;
            len("/path", &f.path, 1, 1_024)
        }),
        "export_minutes" => typed::<ExportMinutesArgs>(a).and_then(|e| {
            len("/folder", &e.folder, 1, 200)?;
            match &e.path {
                Some(p) => len("/path", p, 1, 1_024),
                None => Ok(()),
            }
        }),
        _ => return None,
    };
    Some(result)
}

/// A minutes body is at most 1 MiB (MinutesService.md §E1).
pub(crate) const MAX_BODY_BYTES: usize = 1_048_576;

// ------------------------------------------------------------------------- answers

fn folder_unknown() -> ToolFailure {
    fail(
        "OH_MINUTES_FOLDER_UNKNOWN",
        "This workspace has no folder on this computer called that.",
        false,
    )
}

fn path_invalid() -> ToolFailure {
    fail(
        "OH_MINUTES_PATH_INVALID",
        "That file is not inside the folder.",
        false,
    )
}

/// OH-MIN-6: relative, `/` or `\`, no empty, `.` or `..` segment, no drive or root. The program
/// also refuses a path that leaves the folder once its links are resolved (DSK-MIN-6).
// spec: ordin-desktop/components/OrdinHost#OH-MIN-6
pub fn check_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') || path.contains(':') {
        return false;
    }
    path.split(['/', '\\'])
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// OH-MIN-8: the export file name when Ordin names none.
// spec: ordin-desktop/components/OrdinHost#OH-MIN-8
pub fn export_name(title: &str, format: MinutesExportFormat) -> String {
    let stem: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')' | '.') {
                c
            } else {
                '-'
            }
        })
        .take(120)
        .collect();
    format!("{}.{}", stem.trim(), format.extension())
}

/// The screens' stage words in en-GB (ScreensModule.md SM-MIN-2, SM-MIN-11), for the model.
fn state_words(state: &MinutesJobState) -> String {
    match state {
        MinutesJobState::Queued => "Waiting to start".to_owned(),
        MinutesJobState::WaitingForModel => "Waiting for the speech model".to_owned(),
        MinutesJobState::DownloadingModel {
            received_mb,
            total_mb,
        } => format!("Downloading the speech model — {received_mb} of {total_mb} MB"),
        MinutesJobState::Reading => "Reading the transcript".to_owned(),
        MinutesJobState::Transcribing { done_ms, total_ms } => format!(
            "Turning the recording into text — {} of {} minutes",
            done_ms / 60_000,
            total_ms.div_ceil(60_000)
        ),
        MinutesJobState::WaitingForNotice => "Waiting for your OK".to_owned(),
        MinutesJobState::Writing => "Ordin is writing the minutes".to_owned(),
        MinutesJobState::Done { minutes_id } => format!("Done (minutes_id {minutes_id})"),
        MinutesJobState::TranscriptReady => "Ready for Ordin".to_owned(),
        MinutesJobState::Failed { message, .. } => format!("Failed: {message}"),
        MinutesJobState::Cancelled => "Cancelled".to_owned(),
    }
}

fn source_words(source: MinutesSource) -> &'static str {
    match source {
        MinutesSource::TranscriptFile => "a transcript file",
        MinutesSource::AudioFile => "an audio file",
        MinutesSource::Recording => "a recording",
    }
}

/// `YYYY-MM-DD` of a timestamp's text.
fn day(t: &ordin_types::time::Timestamp) -> String {
    let text = t.to_string();
    text.get(..10).unwrap_or(&text).to_owned()
}

/// One line per utterance, `[hh:mm:ss] Speaker: text` (MinutesService.md §B5).
fn render(transcript: &Transcript) -> String {
    transcript
        .utterances
        .iter()
        .map(|u| {
            let s = u.start_ms / 1_000;
            let at = format!("{:02}:{:02}:{:02}", s / 3_600, (s / 60) % 60, s % 60);
            match &u.speaker {
                Some(speaker) => format!("[{at}] {speaker}: {}", u.text),
                None => format!("[{at}] {}", u.text),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The owner's code and sentence (OH-DEP-1 for minutes: the model may act on `MIN_*` codes).
fn owner(e: &DepError) -> ToolFailure {
    let message = if e.message.is_empty() {
        e.code.clone()
    } else {
        e.message.clone()
    };
    fail(&e.code, &message, e.retryable)
}

impl OrdinHost {
    /// OH-MIN-5: the port the desktop program gives; `None` offers none of the eleven tools.
    pub fn set_minutes_port(&self, port: std::sync::Arc<dyn MinutesPort>) {
        *self.minutes_port.write().unwrap_or_else(|p| p.into_inner()) = Some(port);
    }

    pub(crate) fn minutes_port(&self) -> Option<std::sync::Arc<dyn MinutesPort>> {
        self.minutes_port
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// OH-MIN-6: a folder of the workspace on this computer, by its name.
    async fn local_folder(
        &self,
        ws: WorkspaceId,
        folder: &str,
        writing: bool,
    ) -> Result<PlaceId, ToolFailure> {
        let folders = self
            .ports
            .workspaces
            .agent_folders(ws, self.config.ordin_agent_instance_id)
            .await
            .map_err(|e| crate::tools::dep_fail(&e))?;
        let found = folders
            .into_iter()
            .find(|f| f.name == folder)
            .ok_or_else(folder_unknown)?;
        if writing && found.read_only {
            return Err(fail(
                "OH_MINUTES_FOLDER_READ_ONLY",
                "Ordin can't save into that folder.",
                false,
            ));
        }
        Ok(found.place_id)
    }

    async fn local_file(
        &self,
        ws: WorkspaceId,
        args: &MinutesFileArgs,
    ) -> Result<LocalFile, ToolFailure> {
        if !check_path(&args.path) {
            return Err(path_invalid());
        }
        Ok(LocalFile {
            place_id: self.local_folder(ws, &args.folder, false).await?,
            path: args.path.clone(),
        })
    }

    /// The model consent and the chosen model, for the answer to a started recording job.
    fn started_answer(job: &MinutesJob, transcribing: bool) -> String {
        let mut text = format!(
            "Started (job {}). The person sees it under Minutes › In progress.",
            job.id
        );
        if job.state == MinutesJobState::WaitingForModel {
            text.push_str(" It waits for the speech model: only the person can agree to download it, under Minutes.");
        }
        if transcribing {
            text.push_str(" It can take as long as the recording. Call read_transcription with this job id when it is done.");
        }
        text
    }

    /// One of the eleven tools (OH-MIN-7); `ws` is the session's workspace.
    // spec: ordin-desktop/components/OrdinHost#OH-MIN-7
    pub(crate) async fn minutes_tool(
        &self,
        ws: WorkspaceId,
        tool: &str,
        a: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        let Some(port) = self.minutes_port() else {
            return Err(fail(
                "MCP_TOOL_NOT_OFFERED",
                "That tool is not offered.",
                false,
            ));
        };
        let bad = |p: ArgumentProblem| {
            fail(
                "MCP_ARGUMENTS_INVALID",
                &format!("Wrong arguments for {tool} at {}: {}", p.at, p.reason),
                false,
            )
        };
        match tool {
            "list_minutes" => {
                let l: ListMinutesArgs = typed(&a).map_err(bad)?;
                // `Cursor` is the text it was given (`serde(transparent)`).
                let cursor: Option<Cursor> = match l.cursor {
                    Some(c) => Some(serde_json::from_value(Value::String(c)).map_err(|_| {
                        fail(
                            "MCP_ARGUMENTS_INVALID",
                            "Wrong arguments for list_minutes at /cursor: not a cursor",
                            false,
                        )
                    })?),
                    None => None,
                };
                let page = port.list(ws, cursor).await.map_err(|e| owner(&e))?;
                if page.items.is_empty() {
                    return Ok(out("There are no minutes in this workspace yet.", None));
                }
                let mut text = String::from("Minutes in this workspace, newest first:");
                for m in &page.items {
                    text.push_str(&format!(
                        "\n- {} (minutes_id {}, kept {}, from {})",
                        m.title,
                        m.id,
                        day(&m.kept_at),
                        source_words(m.source)
                    ));
                }
                if let Some(Value::String(next)) = page
                    .next_cursor
                    .as_ref()
                    .and_then(|c| serde_json::to_value(c).ok())
                {
                    text.push_str(&format!(
                        "\nMore: call list_minutes with cursor \"{next}\"."
                    ));
                }
                Ok(out(text, None))
            }
            "read_minutes" => {
                let r: ReadMinutesArgs = typed(&a).map_err(bad)?;
                let m = self.own_minutes(&*port, ws, r.minutes_id).await?;
                Ok(out(
                    format!(
                        "Title: {}\nRevision: {}\nFrom: {}\nKept: {}\n\n{}",
                        m.title,
                        m.revision,
                        m.source_name,
                        day(&m.kept_at),
                        m.body
                    ),
                    Some(json!({ "minutes_id": m.id, "revision": m.revision })),
                ))
            }
            "read_minutes_transcript" => {
                let r: ReadMinutesArgs = typed(&a).map_err(bad)?;
                self.own_minutes(&*port, ws, r.minutes_id).await?;
                let t = port.transcript(r.minutes_id).await.map_err(|e| owner(&e))?;
                Ok(out(render(&t), None))
            }
            "edit_minutes" => {
                let e: EditMinutesArgs = typed(&a).map_err(bad)?;
                self.own_minutes(&*port, ws, e.minutes_id).await?;
                match port.save_edit(e.minutes_id, e.body, e.base_revision).await {
                    Ok(m) => Ok(out(
                        format!("Saved as revision {}.", m.revision),
                        Some(json!({ "revision": m.revision })),
                    )),
                    Err(err) if err.code == "MIN_EDIT_CONFLICT" => {
                        let mut f = owner(&err);
                        f.message.push_str(
                            " Read the minutes again with read_minutes, then make the change on that revision.",
                        );
                        Err(f)
                    }
                    Err(err) => Err(owner(&err)),
                }
            }
            "list_minutes_jobs" => {
                typed::<ListMinutesJobsArgs>(&a).map_err(bad)?;
                let jobs = port.jobs(ws).await.map_err(|e| owner(&e))?;
                if jobs.is_empty() {
                    return Ok(out("Nothing is being worked on.", None));
                }
                let lines: Vec<String> = jobs
                    .iter()
                    .map(|j| {
                        let ordin = if j.purpose == MinutesJobPurpose::Transcript {
                            " (text for Ordin)"
                        } else {
                            ""
                        };
                        format!(
                            "- {}: {}{} — {}",
                            j.id,
                            j.source_name,
                            ordin,
                            state_words(&j.state)
                        )
                    })
                    .collect();
                Ok(out(lines.join("\n"), None))
            }
            "cancel_minutes_job" | "retry_minutes_job" => {
                let j: MinutesJobArgs = typed(&a).map_err(bad)?;
                self.own_job(&*port, ws, j.job_id).await?;
                let job = if tool == "cancel_minutes_job" {
                    port.cancel_job(j.job_id).await
                } else {
                    port.retry(j.job_id).await
                }
                .map_err(|e| owner(&e))?;
                Ok(out(
                    format!(
                        "Done. {} is now: {}.",
                        job.source_name,
                        state_words(&job.state)
                    ),
                    None,
                ))
            }
            "import_minutes" | "transcribe_recording" => {
                let f: MinutesFileArgs = typed(&a).map_err(bad)?;
                let file = self.local_file(ws, &f).await?;
                let transcribing = tool == "transcribe_recording";
                let job = if transcribing {
                    port.transcribe(ws, file).await
                } else {
                    port.import(ws, file).await
                }
                .map_err(|e| owner(&e))?;
                Ok(out(
                    Self::started_answer(&job, transcribing),
                    Some(json!({ "job_id": job.id })),
                ))
            }
            "export_minutes" => {
                let e: ExportMinutesArgs = typed(&a).map_err(bad)?;
                let m = self.own_minutes(&*port, ws, e.minutes_id).await?;
                let path = match e.path {
                    Some(path) => {
                        let wanted = format!(".{}", e.format.extension());
                        if !check_path(&path) || !path.to_lowercase().ends_with(&wanted) {
                            return Err(path_invalid());
                        }
                        path
                    }
                    None => export_name(&m.title, e.format),
                };
                let place_id = self.local_folder(ws, &e.folder, true).await?;
                let written = port
                    .export(e.minutes_id, e.format, LocalFile { place_id, path })
                    .await
                    .map_err(|err| owner(&err))?;
                Ok(out(format!("Saved {written} in {}.", e.folder), None))
            }
            "read_transcription" => {
                let j: MinutesJobArgs = typed(&a).map_err(bad)?;
                let job = self.own_job(&*port, ws, j.job_id).await?;
                if job.purpose != MinutesJobPurpose::Transcript {
                    return Err(fail(
                        "OH_TRANSCRIPTION_NOT_READY",
                        "That job makes minutes, not text for Ordin. Use read_minutes_transcript once it is done.",
                        false,
                    ));
                }
                if job.state != MinutesJobState::TranscriptReady {
                    return Err(fail(
                        "OH_TRANSCRIPTION_NOT_READY",
                        &format!(
                            "That recording is not text yet: {}.",
                            state_words(&job.state)
                        ),
                        false,
                    ));
                }
                let t = port.transcription(j.job_id).await.map_err(|e| owner(&e))?;
                Ok(out(render(&t), None))
            }
            _ => Err(fail(
                "MCP_TOOL_NOT_OFFERED",
                "That tool is not offered.",
                false,
            )),
        }
    }

    /// Minutes of the session's workspace only (OH-MIN-4); another workspace's answer as not found.
    async fn own_minutes(
        &self,
        port: &dyn MinutesPort,
        ws: WorkspaceId,
        id: MinutesId,
    ) -> Result<Minutes, ToolFailure> {
        let m = port.get(id).await.map_err(|e| owner(&e))?;
        if m.workspace_id != ws {
            return Err(fail(
                "MIN_NOT_FOUND",
                "Those minutes are no longer here.",
                false,
            ));
        }
        Ok(m)
    }

    /// A job of the session's workspace only (OH-MIN-4).
    async fn own_job(
        &self,
        port: &dyn MinutesPort,
        ws: WorkspaceId,
        id: MinutesJobId,
    ) -> Result<MinutesJob, ToolFailure> {
        port.jobs(ws)
            .await
            .map_err(|e| owner(&e))?
            .into_iter()
            .find(|j| j.id == id)
            .ok_or_else(|| {
                fail(
                    "MIN_JOB_NOT_FOUND",
                    "That meeting is no longer being worked on.",
                    false,
                )
            })
    }
}
