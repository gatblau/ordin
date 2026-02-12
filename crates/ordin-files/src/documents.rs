//! `read_document`'s reading: the text of a document an agent may read (FileService.md SA-FS-DOC-1
//! to SA-FS-DOC-6). It reads the file in place and changes nothing. The File Service has no edge to
//! the MCP Front (2B-2 D4): the Agent Loader wraps [`DocumentReader`] as the `documents` server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime};

use ordin_rules::PathAccessRequest;
use ordin_types::domain::places::FolderAccess;
use ordin_types::wire::rules::Verdict;
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::extract::{document_text, is_document_kind};
use crate::paths::kind_of;
use crate::service::FileService;
use crate::INDEX_MAX_SOURCE_BYTES;

/// Characters per part: below the MCP Front's 12,000-character trim, so a part arrives whole.
pub const DOC_PART_CHARS: usize = 10_000;
/// The most text kept of one document.
pub const DOC_MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
/// The longest a document's reading may take.
const DOC_TIME_LIMIT: Duration = Duration::from_secs(30);
/// The longest path argument, in bytes.
const MAX_PATH_BYTES: usize = 4096;

/// The last document read by a session, so its next part is not read again.
struct Held {
    path: PathBuf,
    modified: Option<SystemTime>,
    text: Arc<Vec<char>>,
}

/// The reading behind `read_document` (SA-FS-DOC-1), wrapped as a tool by the Agent Loader.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-1
pub struct DocumentReader {
    files: Weak<FileService>,
    held: Mutex<HashMap<RunId, Held>>,
}

/// One call's session facts, copied by the caller from the tool call.
#[derive(Debug, Clone)]
pub struct DocumentCall {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub deadline: Instant,
    pub cancel: CancellationToken,
}

/// One part of a document's text and where to read on (SA-FS-DOC-4).
#[derive(Debug, Clone)]
pub struct DocumentPart {
    pub text: String,
    pub structured: Option<Value>,
}

/// A refused reading: the code, the sentence for the agent, and whether to try again.
#[derive(Debug, Clone)]
pub struct DocumentFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// An argument that is not well formed: its JSON pointer and why.
#[derive(Debug, Clone)]
pub struct DocumentArgumentProblem {
    pub at: String,
    pub reason: String,
}

impl FileService {
    /// The reading behind `read_document`, for the Agent Loader to serve.
    // spec: ordin-desktop/components/FileService#SA-FS-DOC-1
    pub fn document_reader(&self) -> Arc<DocumentReader> {
        Arc::new(DocumentReader {
            files: self.me.clone(),
            held: Mutex::new(HashMap::new()),
        })
    }
}

fn failure(code: &str, message: &str, retryable: bool) -> DocumentFailure {
    DocumentFailure {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    }
}

/// Where the document is: a full path, or a folder of the workspace and the path inside it.
#[derive(Debug)]
enum Target {
    Path(PathBuf),
    /// SA-FS-DOC-6: `place:<place id>:<path>`, as an Attached line gives it.
    // spec: ordin-desktop/components/FileService#SA-FS-DOC-6
    Place(ordin_types::PlaceId, String),
}

/// The arguments after `check_arguments`.
#[derive(Debug)]
struct Args {
    target: Target,
    from: usize,
}

/// SA-FS-DOC-6: `place:<place id>:<path>`, the path relative and never climbing out of the folder.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-6
fn place_target(text: &str) -> Option<Result<Target, &'static str>> {
    let rest = text.strip_prefix("place:")?;
    let Some((id, path)) = rest.split_once(':') else {
        return Some(Err("a place reference is place:<place id>:<path>"));
    };
    let Ok(id) = ordin_types::PlaceId::parse(id) else {
        return Some(Err("the place id is not an id"));
    };
    let relative = Path::new(path);
    let climbs = relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)));
    if path.is_empty() || climbs {
        return Some(Err(
            "the path after the place id must stay inside the folder",
        ));
    }
    Some(Ok(Target::Place(id, path.to_owned())))
}

fn parse(arguments: &Value) -> Result<Args, DocumentArgumentProblem> {
    let problem = |at: &str, reason: &str| DocumentArgumentProblem {
        at: at.to_owned(),
        reason: reason.to_owned(),
    };
    let object = arguments
        .as_object()
        .ok_or_else(|| problem("", "the arguments must be an object"))?;
    if let Some(other) = object.keys().find(|k| *k != "path" && *k != "from") {
        return Err(problem(&format!("/{other}"), "unknown field"));
    }
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| problem("/path", "the path is required, as text"))?;
    if path.is_empty() || path.len() > MAX_PATH_BYTES || path.contains('\0') {
        return Err(problem("/path", "the path must be 1 to 4096 bytes"));
    }
    let target = match place_target(path) {
        Some(Ok(target)) => target,
        Some(Err(reason)) => return Err(problem("/path", reason)),
        None if Path::new(path).is_absolute() => Target::Path(PathBuf::from(path)),
        None => {
            return Err(problem(
                "/path",
                "the path must be absolute, or place:<place id>:<path> from the Attached line",
            ))
        }
    };
    let from = match object.get("from") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| problem("/from", "from must be a whole number of 0 or more"))?,
    };
    Ok(Args { target, from })
}

/// SA-FS-DOC-4: the part of `text` from `from`, with the line saying how to read on.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-4
fn part(text: &[char], from: usize) -> DocumentPart {
    let total = text.len();
    if from >= total {
        return DocumentPart {
            text: format!("[No more text: the document has {total} characters.]"),
            structured: Some(json!({"characters": total, "from": from, "next": null})),
        };
    }
    let end = (from + DOC_PART_CHARS).min(total);
    let mut out: String = text[from..end].iter().collect();
    let next = (end < total).then_some(end);
    if let Some(next) = next {
        out.push_str(&format!(
            "\n\n[More text follows: call read_document again with from: {next}. The document has {total} characters.]"
        ));
    }
    DocumentPart {
        text: out,
        structured: Some(json!({"characters": total, "from": from, "next": next})),
    }
}

impl DocumentReader {
    /// SA-FS-DOC-1: the arguments `read_document` takes, checked before the call.
    pub fn check(arguments: &Value) -> Result<(), DocumentArgumentProblem> {
        parse(arguments).map(|_| ())
    }

    /// SA-FS-DOC-3 and SA-FS-DOC-4: the part of the document the arguments name.
    pub async fn read(
        &self,
        call: &DocumentCall,
        arguments: &Value,
    ) -> Result<DocumentPart, DocumentFailure> {
        let args =
            parse(arguments).map_err(|p| failure("MCP_ARGUMENTS_INVALID", &p.reason, false))?;
        self.read_args(call, args).await
    }

    /// The session ended: the text held for it is dropped.
    pub fn session_closed(&self, run_id: RunId) {
        crate::service::lock(&self.held).remove(&run_id);
    }

    fn files(&self) -> Result<Arc<FileService>, DocumentFailure> {
        self.files.upgrade().ok_or_else(|| {
            failure(
                "FILE_DOC_UNREADABLE",
                "Documents cannot be read just now; try again shortly.",
                true,
            )
        })
    }

    /// SA-FS-DOC-3 steps 1 to 6, then SA-FS-DOC-4.
    // spec: ordin-desktop/components/FileService#SA-FS-DOC-3
    async fn read_args(
        &self,
        ctx: &DocumentCall,
        args: Args,
    ) -> Result<DocumentPart, DocumentFailure> {
        let files = self.files()?;
        // 1. The real file, symbolic links resolved, so the check below sees where it is.
        let given = match &args.target {
            Target::Path(path) => path.clone(),
            Target::Place(id, path) => match files.place(*id).await {
                Ok(place) => Path::new(&place.path).join(path),
                Err(_) => {
                    return Err(failure(
                        "FILE_NOT_FOUND",
                        "There is no folder with that place id. Copy the place reference from the Attached line.",
                        false,
                    ))
                }
            },
        };
        let real = crate::versions::blocking(move || std::fs::canonicalize(&given))
            .await
            .ok()
            .and_then(Result::ok)
            .ok_or_else(|| {
                failure(
                    "FILE_NOT_FOUND",
                    "There is no file at that path. Check the path from the Attached line.",
                    false,
                )
            })?;
        // 2. The agent's own folder or a folder the person allowed, read access (Rules K.5).
        let decision = files.rules.check_path_access(&PathAccessRequest {
            workspace_id: Some(ctx.workspace_id),
            agent: Some(ctx.agent_instance_id),
            crew_charter_id: None,
            path: real.clone(),
            access: FolderAccess::Read,
        });
        if !matches!(decision.verdict, Verdict::Allow) {
            crate::log::document_refused(ctx.run_id, "FILE_NOT_GRANTED");
            return Err(failure(
                "FILE_NOT_GRANTED",
                "This file is outside the folders you may read. Ask the person to allow its folder.",
                false,
            ));
        }
        // 3. A regular file of a kind with text, at most 50 MB.
        let probe = real.clone();
        let meta = crate::versions::blocking(move || std::fs::metadata(&probe))
            .await
            .ok()
            .and_then(Result::ok)
            .filter(std::fs::Metadata::is_file)
            .ok_or_else(|| failure("FILE_NOT_FOUND", "There is no file at that path.", false))?;
        let name = real
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let kind = kind_of(name);
        if !is_document_kind(&kind) {
            return Err(failure(
                "FILE_DOC_KIND_UNSUPPORTED",
                "This kind of file has no text to read. read_document reads PDF, Word, PowerPoint, Excel, Markdown and text files.",
                false,
            ));
        }
        if meta.len() > INDEX_MAX_SOURCE_BYTES {
            return Err(failure(
                "FILE_TOO_LARGE",
                "This file is larger than 50 MB, too large to read. Ask the person for a smaller copy.",
                false,
            ));
        }
        let modified = meta.modified().ok();
        // 4. The text held for this session when it is the same file, unchanged.
        let held = {
            let held = crate::service::lock(&self.held);
            held.get(&ctx.run_id)
                .filter(|h| h.path == real && h.modified == modified)
                .map(|h| h.text.clone())
        };
        let text = match held {
            Some(text) => text,
            None => {
                let text = Arc::new(self.extract(&files, ctx, real.clone(), kind).await?);
                crate::service::lock(&self.held).insert(
                    ctx.run_id,
                    Held {
                        path: real,
                        modified,
                        text: text.clone(),
                    },
                );
                text
            }
        };
        Ok(part(&text, args.from))
    }

    /// Step 5: the extraction on a blocking thread, one of the File Service's two, within the
    /// time limit and the call's deadline. Step 6: no text is `FILE_DOC_NO_TEXT`.
    async fn extract(
        &self,
        files: &FileService,
        ctx: &DocumentCall,
        path: PathBuf,
        kind: String,
    ) -> Result<Vec<char>, DocumentFailure> {
        let unreadable = || {
            failure(
                "FILE_DOC_UNREADABLE",
                "This document could not be read: the file may be damaged or protected. Ask the person for a text copy.",
                false,
            )
        };
        let _slot = files
            .extraction
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| unreadable())?;
        let started = Instant::now();
        let limit = DOC_TIME_LIMIT.min(ctx.deadline.saturating_duration_since(started));
        let work = tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(&path).map_err(|_| None)?;
            document_text(&kind, &bytes, DOC_MAX_TEXT_BYTES).map_err(|f| Some(f.class()))
        });
        let text = tokio::select! {
            joined = tokio::time::timeout(limit, work) => match joined {
                Err(_) => {
                    crate::log::document_refused(ctx.run_id, "timed_out");
                    return Err(failure(
                        "FILE_DOC_UNREADABLE",
                        "This document took too long to read. Ask the person for a text copy.",
                        false,
                    ));
                }
                Ok(Err(_)) => return Err(unreadable()),
                Ok(Ok(Err(class))) => {
                    crate::log::document_refused(ctx.run_id, class.unwrap_or("io"));
                    return Err(unreadable());
                }
                Ok(Ok(Ok(text))) => text.unwrap_or_default(),
            },
            () = ctx.cancel.cancelled() => {
                return Err(failure("FILE_DOC_UNREADABLE", "The reading was cancelled.", true));
            }
        };
        if text.trim().is_empty() {
            crate::log::document_refused(ctx.run_id, "no_text");
            return Err(failure(
                "FILE_DOC_NO_TEXT",
                "This document holds no text that can be read (it may be scanned pages). Ask the person for a text copy.",
                false,
            ));
        }
        crate::log::document_read(ctx.run_id, started.elapsed());
        Ok(text.chars().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_ends_with_where_to_read_on() {
        let text: Vec<char> = "a".repeat(DOC_PART_CHARS + 5).chars().collect();
        let first = part(&text, 0);
        assert!(first.text.starts_with(&"a".repeat(DOC_PART_CHARS)));
        assert!(first.text.contains(&format!("from: {DOC_PART_CHARS}")));
        assert_eq!(first.structured.as_ref().unwrap()["next"], DOC_PART_CHARS);
        let last = part(&text, DOC_PART_CHARS);
        assert_eq!(last.text, "aaaaa");
        assert!(last.structured.as_ref().unwrap()["next"].is_null());
        assert!(part(&text, DOC_PART_CHARS + 5)
            .text
            .starts_with("[No more text"));
    }

    #[test]
    fn the_arguments_are_checked() {
        assert!(parse(&json!({"path": "/a/b.pdf"})).is_ok());
        assert_eq!(
            parse(&json!({"path": "/a/b.pdf", "from": 20}))
                .unwrap()
                .from,
            20
        );
        assert_eq!(parse(&json!({"path": "b.pdf"})).unwrap_err().at, "/path");
        let id = ordin_types::PlaceId::new();
        assert!(matches!(
            parse(&json!({"path": format!("place:{id}:docs/b.pdf")})).unwrap().target,
            Target::Place(p, ref path) if p == id && path == "docs/b.pdf"
        ));
        for bad in [
            format!("place:{id}:../b.pdf"),
            format!("place:{id}:/etc/b.pdf"),
            format!("place:{id}:"),
            "place:not-an-id:b.pdf".to_owned(),
        ] {
            assert_eq!(
                parse(&json!({"path": bad})).unwrap_err().at,
                "/path",
                "{bad}"
            );
        }
        assert_eq!(parse(&json!({})).unwrap_err().at, "/path");
        assert_eq!(
            parse(&json!({"path": "/a", "from": -1})).unwrap_err().at,
            "/from"
        );
        assert_eq!(
            parse(&json!({"path": "/a", "page": 1})).unwrap_err().at,
            "/page"
        );
    }
}
