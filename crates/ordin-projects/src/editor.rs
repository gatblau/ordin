//! The relay between the code editor and the language servers (ADR-055, ADR-056; Internal Logic 9.5).

use std::path::Path;

use ordin_types::ProjectId;
use serde_json::Value;

use crate::error::ProjectError;
use crate::lsp::{path_of, EditorState, InstanceKey};
use crate::service::{lock, ProjectService};
use crate::types::{EditorSession, StreamId, StreamPayload};

/// The line of an editor with no server (ADR-055).
const NO_SERVER_LINE: &str = "highlighted · no language server";

/// The line of an editor whose repository waits for the person's answer on language servers.
const WAITING_LINE: &str = "highlighted · language server waiting for your answer";

/// The language of a file by its extension (*chosen here*: the four first languages of ADR-024).
pub(crate) fn language_of(path: &str) -> Option<&'static str> {
    match Path::new(path).extension()?.to_str()? {
        "go" => Some("go"),
        "rs" => Some("rust"),
        "java" => Some("java"),
        "ts" | "tsx" => Some("typescript"),
        _ => None,
    }
}

fn language_title(language: &str) -> &str {
    match language {
        "go" => "Go",
        "rust" => "Rust",
        "java" => "Java",
        "typescript" => "TypeScript",
        other => other,
    }
}

/// Every document URI of a message lies inside `folder`.
fn documents_inside(message: &Value, folder: &Path) -> bool {
    let params = &message["params"];
    let uri = params["textDocument"]["uri"]
        .as_str()
        .or_else(|| params["data"]["uri"].as_str());
    match uri {
        None => true,
        Some(uri) => path_of(uri).is_some_and(|path| crate::git::under_root(&path, folder)),
    }
}

impl ProjectService {
    /// Internal Logic 9.5. A failed or absent server still opens the editor, with the line that
    /// says so.
    // spec: ordin-desktop/components/ProjectService#ADR-055
    pub async fn editor_open(
        &self,
        project_id: ProjectId,
        path: &str,
    ) -> Result<EditorSession, ProjectError> {
        let row = self.open_row(project_id).await?;
        let folder = self.place_folder(row.project.place_id).await?;
        let stream = StreamId(uuid::Uuid::now_v7());
        let language = language_of(path).map(str::to_owned);
        let mut server_line = NO_SERVER_LINE.to_owned();
        let answer = self.language_servers_answer(project_id).await?;
        let asks_to_start_servers = language.is_some() && answer.is_none();
        if asks_to_start_servers {
            server_line = WAITING_LINE.to_owned();
        }
        if let Some(language) = language.as_ref().filter(|_| answer == Some(true)) {
            let key = InstanceKey::Editor(project_id, language.clone());
            match self.instance(key, &folder, language).await {
                Ok(instance) => {
                    instance.attach(stream);
                    server_line = format!(
                        "{} · {} · Running",
                        language_title(language),
                        instance.server
                    );
                }
                Err(ProjectError::DatastoreUnavailable) => {
                    return Err(ProjectError::DatastoreUnavailable)
                }
                Err(_) => {}
            }
        }
        lock(&self.lsp.sessions).insert(
            stream,
            EditorState {
                project: project_id,
                folder,
                language: language.clone(),
            },
        );
        Ok(EditorSession {
            stream,
            language,
            server_line,
            asks_to_start_servers,
        })
    }

    /// Internal Logic 9.5: only `EDITOR_LSP_METHODS` pass and document URIs stay inside the project
    /// folder; the instance restarts here when a crash ended it.
    // spec: ordin-desktop/components/ProjectService#ADR-056
    pub async fn lsp_send(
        &self,
        session: StreamId,
        message: serde_json::Value,
    ) -> Result<(), ProjectError> {
        let method = message["method"].as_str().unwrap_or_default();
        if !crate::EDITOR_LSP_METHODS.contains(&method) {
            return Err(ProjectError::LspMethodNotAllowed);
        }
        let state = lock(&self.lsp.sessions)
            .get(&session)
            .cloned()
            .ok_or(ProjectError::LspUnavailable)?;
        // A document outside the project has no code of its own; it is refused with the
        // code of the same check's other half.
        if !documents_inside(&message, &state.folder) {
            return Err(ProjectError::LspMethodNotAllowed);
        }
        let language = state.language.ok_or(ProjectError::LspUnavailable)?;
        let key = InstanceKey::Editor(state.project, language.clone());
        let instance = self.instance(key, &state.folder, &language).await?;
        instance.attach(session);
        instance.relay(session, message)
    }

    pub async fn editor_close(&self, session: StreamId) {
        let state = lock(&self.lsp.sessions).remove(&session);
        if let Some(EditorState {
            project,
            language: Some(language),
            ..
        }) = state
        {
            let key = InstanceKey::Editor(project, language);
            if let Some(instance) = lock(&self.lsp.instances).get(&key) {
                instance.detach(session);
            }
        }
        self.streams.send(
            session,
            StreamPayload::Closed {
                reason: "editor_closed".to_owned(),
            },
        );
    }
}
