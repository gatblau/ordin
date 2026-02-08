//! The `documents` server: `read_document`, the text of a document an agent may read
//! (FileService.md SA-FS-DOC-1 to SA-FS-DOC-5). It reads the file in place and changes nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime};

use async_trait::async_trait;
use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolFailure, ToolKey,
    ToolOutput, ToolProvider, ToolSpec,
};
use ordin_rules::PathAccessRequest;
use ordin_types::domain::places::FolderAccess;
use ordin_types::wire::rules::Verdict;
use ordin_types::RunId;
use serde_json::{json, Value};

use crate::extract::{document_text, is_document_kind};
use crate::paths::kind_of;
use crate::service::FileService;
use crate::INDEX_MAX_SOURCE_BYTES;

const PROVIDER_ID: &str = "file-service-documents";
pub(crate) const DOC_SERVER: &str = "documents";
const READ_DOCUMENT: &str = "read_document";
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

/// The provider of the `documents` server (SA-FS-DOC-1).
// spec: ordin-desktop/components/FileService#SA-FS-DOC-1
pub struct DocumentTools {
    files: Weak<FileService>,
    held: Mutex<HashMap<RunId, Held>>,
}

impl FileService {
    /// The `documents` server's provider, registered with the MCP Front by the program.
    // spec: ordin-desktop/components/FileService#SA-FS-DOC-1
    pub fn document_tool_provider(&self) -> Arc<dyn ToolProvider> {
        Arc::new(DocumentTools {
            files: self.me.clone(),
            held: Mutex::new(HashMap::new()),
        })
    }
}

fn failure(code: &str, message: &str, retryable: bool) -> ToolFailure {
    ToolFailure {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    }
}

/// The arguments after `check_arguments`.
#[derive(Debug)]
struct Args {
    path: PathBuf,
    from: usize,
}

fn parse(arguments: &Value) -> Result<Args, ArgumentProblem> {
    let problem = |at: &str, reason: &str| ArgumentProblem {
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
    if !Path::new(path).is_absolute() {
        return Err(problem("/path", "the path must be absolute"));
    }
    let from = match object.get("from") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| problem("/from", "from must be a whole number of 0 or more"))?,
    };
    Ok(Args {
        path: PathBuf::from(path),
        from,
    })
}

/// SA-FS-DOC-4: the part of `text` from `from`, with the line saying how to read on.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-4
fn part(text: &[char], from: usize) -> ToolOutput {
    let total = text.len();
    if from >= total {
        return ToolOutput {
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
    ToolOutput {
        text: out,
        structured: Some(json!({"characters": total, "from": from, "next": next})),
    }
}

impl DocumentTools {
    fn files(&self) -> Result<Arc<FileService>, ToolFailure> {
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
    async fn read(&self, ctx: &ToolCallContext, args: Args) -> Result<ToolOutput, ToolFailure> {
        let files = self.files()?;
        // 1. The real file, symbolic links resolved, so the check below sees where it is.
        let given = args.path.clone();
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
        let name = real.file_name().and_then(|n| n.to_str()).unwrap_or_default();
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
        ctx: &ToolCallContext,
        path: PathBuf,
        kind: String,
    ) -> Result<Vec<char>, ToolFailure> {
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

#[async_trait]
impl ToolProvider for DocumentTools {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn servers(&self) -> Vec<ServerSpec> {
        let Ok(name) = ServerName::new(DOC_SERVER) else {
            return Vec::new();
        };
        vec![ServerSpec {
            name,
            tools: vec![ToolSpec {
                name: READ_DOCUMENT.to_owned(),
                description: "Read the text of a PDF, Word, PowerPoint, Excel, Markdown or text file, in parts."
                    .to_owned(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "The file's absolute path, as on the Attached line."},
                        "from": {"type": "integer", "minimum": 0, "description": "The character to start at; 0 for the first part."}
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
                argument_form: "read_document(path: \"<absolute path>\", from: <0, or the next offset given>)"
                    .to_owned(),
                sandbox_callable: true,
                plain: "Reads the text of documents in the folders you allow.".to_owned(),
            }],
        }]
    }

    // spec: ordin-desktop/components/FileService#SA-FS-DOC-1
    async fn offer(&self, _grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let Ok(server) = ServerName::new(DOC_SERVER) else {
            return Ok(Vec::new());
        };
        Ok(vec![ToolKey {
            server,
            tool: READ_DOCUMENT.to_owned(),
        }])
    }

    fn check_arguments(&self, key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        if key.tool != READ_DOCUMENT {
            return Err(ArgumentProblem {
                at: String::new(),
                reason: "unknown tool".to_owned(),
            });
        }
        parse(arguments).map(|_| ())
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        if key.tool != READ_DOCUMENT {
            return Err(failure(
                "MCP_TOOL_NOT_OFFERED",
                "This tool is not offered to this session.",
                false,
            ));
        }
        let args = parse(&arguments).map_err(|p| failure("MCP_ARGUMENTS_INVALID", &p.reason, false))?;
        self.read(&ctx, args).await
    }

    async fn session_closed(&self, run_id: RunId) {
        crate::service::lock(&self.held).remove(&run_id);
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
        assert!(part(&text, DOC_PART_CHARS + 5).text.starts_with("[No more text"));
    }

    #[test]
    fn the_arguments_are_checked() {
        assert!(parse(&json!({"path": "/a/b.pdf"})).is_ok());
        assert_eq!(parse(&json!({"path": "/a/b.pdf", "from": 20})).unwrap().from, 20);
        assert_eq!(parse(&json!({"path": "b.pdf"})).unwrap_err().at, "/path");
        assert_eq!(parse(&json!({})).unwrap_err().at, "/path");
        assert_eq!(parse(&json!({"path": "/a", "from": -1})).unwrap_err().at, "/from");
        assert_eq!(parse(&json!({"path": "/a", "page": 1})).unwrap_err().at, "/page");
    }
}
