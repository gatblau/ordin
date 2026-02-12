//! The `documents` server (FileService.md SA-FS-DOC-1): `read_document` served through the MCP Front
//! over the File Service's [`DocumentReader`]. The File Service has no edge to the MCP Front (2B-2 D4,
//! ArchitectureTest.md), so the program wraps the reading as the provider, as it wires every other one.

use std::sync::Arc;

use async_trait::async_trait;
use ordin_files::{DocumentCall, DocumentFailure, DocumentPart, DocumentReader, FileService};
use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolFailure, ToolKey,
    ToolOutput, ToolProvider, ToolSpec,
};
use ordin_types::RunId;
use serde_json::{json, Value};

const PROVIDER_ID: &str = "file-service-documents";
const DOC_SERVER: &str = "documents";
const READ_DOCUMENT: &str = "read_document";

/// The provider of the `documents` server.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-1
pub struct DocumentTools {
    reader: Arc<DocumentReader>,
}

/// The `documents` server's provider over `files`, for the program to register with the MCP Front.
// spec: ordin-desktop/components/FileService#SA-FS-DOC-1
pub fn document_tool_provider(files: &FileService) -> Arc<dyn ToolProvider> {
    Arc::new(DocumentTools {
        reader: files.document_reader(),
    })
}

fn failure(code: &str, message: &str) -> ToolFailure {
    ToolFailure {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable: false,
    }
}

fn output(part: DocumentPart) -> ToolOutput {
    ToolOutput {
        text: part.text,
        structured: part.structured,
    }
}

fn refused(failure: DocumentFailure) -> ToolFailure {
    ToolFailure {
        code: failure.code,
        message: failure.message,
        retryable: failure.retryable,
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
                        "path": {"type": "string", "description": "The file's absolute path, or place:<place id>:<path> as on the Attached line."},
                        "from": {"type": "integer", "minimum": 0, "description": "The character to start at; 0 for the first part."}
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
                argument_form: "read_document(path: \"<absolute path> | place:<place id>:<path>\", from: <0, or the next offset given>)"
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
        DocumentReader::check(arguments).map_err(|p| ArgumentProblem {
            at: p.at,
            reason: p.reason,
        })
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
            ));
        }
        let call = DocumentCall {
            run_id: ctx.run_id,
            workspace_id: ctx.workspace_id,
            agent_instance_id: ctx.agent_instance_id,
            deadline: ctx.deadline,
            cancel: ctx.cancel.clone(),
        };
        self.reader
            .read(&call, &arguments)
            .await
            .map(output)
            .map_err(refused)
    }

    async fn session_closed(&self, run_id: RunId) {
        self.reader.session_closed(run_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: ordin-desktop/components/FileService#SA-FS-DOC-1
    #[test]
    fn a_failure_keeps_its_code_sentence_and_retry() {
        let out = refused(DocumentFailure {
            code: "FILE_DOC_UNREADABLE".to_owned(),
            message: "Try again shortly.".to_owned(),
            retryable: true,
        });
        assert_eq!(
            (out.code.as_str(), out.message.as_str(), out.retryable),
            ("FILE_DOC_UNREADABLE", "Try again shortly.", true)
        );
        let part = output(DocumentPart {
            text: "words".to_owned(),
            structured: Some(json!({"next": null})),
        });
        assert_eq!(part.text, "words");
        assert!(part.structured.is_some());
    }
}
