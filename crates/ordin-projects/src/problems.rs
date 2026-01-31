//! New problems (ADR-054, SA-LSP-03; Internal Logic 6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::editor::language_of;
use crate::error::ProjectError;
use crate::git;
use crate::lsp::{uri_of, Instance, InstanceKey};
use crate::service::{lock, ProjectService};
use crate::store::{self, TaskRow};
use crate::types::Problem;

/// One diagnostic, read from a server's message.
#[derive(Debug, Clone)]
pub(crate) struct Diagnostic {
    pub line: u32,
    pub character: u32,
    pub severity: String,
    pub code: Option<String>,
    pub message: String,
}

fn severity_word(value: &Value) -> &'static str {
    match value.as_u64() {
        Some(1) => "error",
        Some(2) => "warning",
        Some(3) => "information",
        Some(4) => "hint",
        _ => "error",
    }
}

fn diagnostic_of(value: &Value) -> Diagnostic {
    Diagnostic {
        line: value["range"]["start"]["line"].as_u64().unwrap_or(0) as u32,
        character: value["range"]["start"]["character"].as_u64().unwrap_or(0) as u32,
        severity: severity_word(&value["severity"]).to_owned(),
        code: match &value["code"] {
            Value::String(code) => Some(code.clone()),
            Value::Number(code) => Some(code.to_string()),
            _ => None,
        },
        message: value["message"].as_str().unwrap_or_default().to_owned(),
    }
}

fn contains(range: &Value, line: u32, character: u32) -> bool {
    let at = |point: &Value| {
        (
            point["line"].as_u64().unwrap_or(0),
            point["character"].as_u64().unwrap_or(0),
        )
    };
    let here = (u64::from(line), u64::from(character));
    at(&range["start"]) <= here && here <= at(&range["end"])
}

/// The innermost symbol containing the point: the names from the outermost down, joined.
pub(crate) fn enclosing_symbol(symbols: &Value, line: u32, character: u32) -> String {
    fn walk(symbols: &[Value], line: u32, character: u32, path: &mut Vec<String>) {
        for symbol in symbols {
            // A `DocumentSymbol` has `range`; a `SymbolInformation` has `location.range`.
            let range = if symbol["range"].is_object() {
                &symbol["range"]
            } else {
                &symbol["location"]["range"]
            };
            if contains(range, line, character) {
                path.push(symbol["name"].as_str().unwrap_or_default().to_owned());
                if let Some(children) = symbol["children"].as_array() {
                    walk(children, line, character, path);
                }
                return;
            }
        }
    }
    let mut path = Vec::new();
    if let Some(list) = symbols.as_array() {
        walk(list, line, character, &mut path);
    }
    path.join("/")
}

/// What identifies a diagnostic across the task and the base: code (message when no code),
/// severity and enclosing symbol, for one file.
fn identity(diagnostic: &Diagnostic, symbols: &Value) -> (String, String, String) {
    (
        diagnostic
            .code
            .clone()
            .unwrap_or_else(|| diagnostic.message.clone()),
        diagnostic.severity.clone(),
        enclosing_symbol(symbols, diagnostic.line, diagnostic.character),
    )
}

/// Reads the files, waits for the server and returns each file's diagnostics with its symbols.
async fn collect(
    instance: &Arc<Instance>,
    language: &str,
    files: &[String],
    settle: Duration,
    limit: Duration,
    request: Duration,
) -> Result<BTreeMap<String, (Vec<Diagnostic>, Value)>, ProjectError> {
    let mut uris = Vec::new();
    for file in files {
        let path = instance.root.join(file);
        let uri = uri_of(&path);
        // A file the task deleted has nothing to open.
        if let Ok(text) = tokio::fs::read_to_string(&path).await {
            instance.open_document(&uri, language, &text)?;
            uris.push((file.clone(), uri));
        }
    }
    instance.settled(settle, limit).await;
    let mut out = BTreeMap::new();
    for (file, uri) in uris {
        let diagnostics: Vec<Diagnostic> = instance
            .diagnostics_of(&uri)
            .iter()
            .map(diagnostic_of)
            .collect();
        let symbols = instance
            .request(
                "textDocument/documentSymbol",
                json!({"textDocument": {"uri": uri}}),
                request,
            )
            .await?;
        out.insert(file, (diagnostics, symbols));
    }
    Ok(out)
}

impl ProjectService {
    fn base_worktree_path(&self, row: &TaskRow) -> PathBuf {
        self.data_root()
            .join("worktrees")
            .join(row.task.project_id.to_string())
            .join(format!("base-{}", row.task.id))
    }

    /// Removes the task's base worktree when there is one.
    pub(crate) async fn remove_base_worktree(
        &self,
        folder: &Path,
        row: &TaskRow,
    ) -> Result<(), ProjectError> {
        let path = self.base_worktree_path(row);
        if !path.exists() {
            return Ok(());
        }
        let (source, name) = (folder.to_path_buf(), format!("base-{}", row.task.id));
        git::blocking(move || git::remove_worktree(&source, &name, &path, None)).await
    }

    /// The new problems of one language, comparing the task's worktree with its base.
    async fn language_problems(
        &self,
        row: &TaskRow,
        language: &str,
        files: &[String],
        base: &Path,
    ) -> Result<Vec<Problem>, ProjectError> {
        let (settle, limit, request) = {
            let tunables = lock(&self.tunables);
            (
                tunables.diagnostics_settle,
                tunables.lsp_start_timeout,
                tunables.lsp_request_timeout,
            )
        };
        let worktree = PathBuf::from(&row.task.worktree_path);
        let task_key = InstanceKey::Task(row.task.id, language.to_owned());
        let task = self.instance(task_key, &worktree, language).await?;
        let base_key = InstanceKey::Base(row.task.id, language.to_owned());
        let base_instance = self.instance(base_key.clone(), base, language).await?;
        let found = async {
            let in_task = collect(&task, language, files, settle, limit, request).await?;
            let in_base = collect(&base_instance, language, files, settle, limit, request).await?;
            Ok::<_, ProjectError>((in_task, in_base))
        }
        .await;
        self.stop_instance(&base_key).await;
        let (in_task, in_base) = found?;
        let mut problems = Vec::new();
        for (file, (diagnostics, symbols)) in in_task {
            let known: Vec<_> = in_base
                .get(&file)
                .map(|(list, base_symbols)| {
                    list.iter().map(|d| identity(d, base_symbols)).collect()
                })
                .unwrap_or_default();
            for diagnostic in diagnostics {
                if !known.contains(&identity(&diagnostic, &symbols)) {
                    problems.push(Problem {
                        file: file.clone(),
                        line: diagnostic.line,
                        severity: diagnostic.severity,
                        code: diagnostic.code,
                        message: diagnostic.message,
                    });
                }
            }
        }
        Ok(problems)
    }

    /// Internal Logic 6.1 and 6.2. A language not confirmed for worktrees (C-33), or whose server
    /// fails or may not run, adds no line and marks the task unchecked. Returns the count stored.
    // spec: ordin-desktop/components/ProjectService#C-33
    // spec: ordin-desktop/components/ProjectService#SA-LSP-03
    // spec: ordin-desktop/components/ProjectService#ADR-054
    pub(crate) async fn find_new_problems(&self, row: &TaskRow, files: &[String]) -> u32 {
        let confirmed = lock(&self.tunables).worktree_confirmed.clone();
        let mut by_language: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        let mut unchecked = false;
        for file in files {
            match language_of(file) {
                Some(language) if confirmed.contains(language) => {
                    by_language.entry(language).or_default().push(file.clone());
                }
                Some(_) => unchecked = true,
                None => {}
            }
        }
        let count = if by_language.is_empty() {
            0
        } else {
            let (count, failed) = self.stored_problems(row, &by_language).await;
            unchecked |= failed;
            count
        };
        if let Err(error) = store::set_problems_unchecked(&self.pool, row.task.id, unchecked).await
        {
            crate::log::background_failed("new_problems", error.code());
        }
        count
    }

    /// The count stored, and whether some language could not be read.
    async fn stored_problems(
        &self,
        row: &TaskRow,
        by_language: &BTreeMap<&str, Vec<String>>,
    ) -> (u32, bool) {
        match self.problems_with_base(row, by_language).await {
            Ok((problems, failed)) => {
                let count = problems.len();
                if let Err(error) =
                    store::set_new_problems(&self.pool, row.task.id, &problems).await
                {
                    crate::log::background_failed("new_problems", error.code());
                }
                (u32::try_from(count).unwrap_or(u32::MAX), failed)
            }
            Err(error) => {
                crate::log::background_failed("new_problems", error.code());
                (0, true)
            }
        }
    }

    async fn problems_with_base(
        &self,
        row: &TaskRow,
        by_language: &BTreeMap<&str, Vec<String>>,
    ) -> Result<(Vec<Problem>, bool), ProjectError> {
        let project = store::project(&self.pool, row.task.project_id)
            .await?
            .ok_or(ProjectError::NotFound)?;
        let folder = self.place_folder(project.project.place_id).await?;
        let base = self.base_worktree_path(row);
        let (source, name, commit, path) = (
            folder.clone(),
            format!("base-{}", row.task.id),
            row.task.base_commit.clone(),
            base.clone(),
        );
        git::blocking(move || git::add_base_worktree(&source, &name, &commit, &path)).await?;
        let mut problems = Vec::new();
        let mut failed = false;
        for (language, files) in by_language {
            match self.language_problems(row, language, files, &base).await {
                Ok(found) => problems.extend(found),
                Err(error) => {
                    failed = true;
                    crate::log::background_failed("language_problems", error.code());
                }
            }
        }
        self.remove_base_worktree(&folder, row).await?;
        Ok((problems, failed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(name: &str, from: u64, to: u64, children: Value) -> Value {
        json!({
            "name": name,
            "range": {"start": {"line": from, "character": 0}, "end": {"line": to, "character": 0}},
            "children": children,
        })
    }

    #[test]
    fn the_innermost_symbol_names_the_place() {
        let symbols = json!([
            symbol("Rates", 0, 10, json!([symbol("fetch", 2, 5, json!([]))])),
            symbol("other", 11, 12, json!([])),
        ]);
        assert_eq!(enclosing_symbol(&symbols, 3, 0), "Rates/fetch");
        assert_eq!(enclosing_symbol(&symbols, 8, 0), "Rates");
        assert_eq!(enclosing_symbol(&symbols, 20, 0), "");
        let information = json!([{
            "name": "fetch",
            "location": {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 4, "character": 0}}},
        }]);
        assert_eq!(enclosing_symbol(&information, 2, 0), "fetch");
    }

    #[test]
    fn a_diagnostic_keeps_its_code_as_text() {
        let read = diagnostic_of(&json!({
            "range": {"start": {"line": 4, "character": 2}},
            "severity": 2, "code": 1002, "message": "m",
        }));
        assert_eq!(read.code.as_deref(), Some("1002"));
        assert_eq!(read.severity, "warning");
        assert_eq!(read.line, 4);
        assert_eq!(severity_word(&json!(3)), "information");
        assert_eq!(severity_word(&json!(4)), "hint");
        assert_eq!(severity_word(&Value::Null), "error");
    }
}
