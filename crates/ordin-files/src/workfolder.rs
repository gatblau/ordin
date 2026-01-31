//! The hosted agent's work folder (FileService.md Internal Logic 10 and 13; the agent profile only).
//! Files live in `work_folder_files`, one namespace per agent; the bundle is read from a folder on
//! local disk and is read-only. Runs on SQLite and on PostgreSQL.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use ordin_journal::Datastore;
use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
use ordin_types::time::Timestamp;
use regex::RegexBuilder;
use sqlx::Row;

use crate::error::FileError;
use crate::log;
use crate::service::ts;
use crate::types::{AgentRefusal, GrepMatch, WorkFileEntry, WorkFolderConfig};
use crate::{WF_BUNDLE_DIR_PREFIXES, WF_BUNDLE_FILES, WF_MAX_FILE_BYTES, WF_MAX_PATH_CHARS};

/// WR-FileTree `MAX_FILE_BYTES`; equals the constant `WF_MAX_FILE_BYTES`.
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;

const DEFAULT_GREP_COUNT: u32 = 100;
const UNREACHABLE: &str = "Error: the work folder cannot be reached";

fn invalid_path() -> FileError {
    FileError::WorkFolder("WF_INVALID_PATH")
}

/// The work-folder path rule of Internal Logic 10.2: starts with `/`, holds no NUL, no `..`
/// segment and no `~`, at most 1,024 characters; collapses `//` and `.`. A trailing `/` is kept
/// (it names a folder).
pub fn normalise_path(path: &str) -> Result<String, FileError> {
    if !path.starts_with('/')
        || path.contains('\0')
        || path.contains('~')
        || path.chars().count() > WF_MAX_PATH_CHARS
    {
        return Err(invalid_path());
    }
    let mut kept: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return Err(invalid_path()),
            other => kept.push(other),
        }
    }
    let mut out = format!("/{}", kept.join("/"));
    if path.ends_with('/') && out != "/" {
        out.push('/');
    }
    if out.chars().count() > WF_MAX_PATH_CHARS {
        return Err(invalid_path());
    }
    Ok(out)
}

/// True for a normalised path in `WF_BUNDLE_FILES` or under a `WF_BUNDLE_DIR_PREFIXES` entry (or
/// equal to one without its trailing `/`).
pub fn is_bundle_path(normalised: &str) -> bool {
    WF_BUNDLE_FILES.contains(&normalised)
        || WF_BUNDLE_DIR_PREFIXES
            .iter()
            .any(|p| normalised.starts_with(p) || normalised == p.trim_end_matches('/'))
}

/// A path without a trailing `/` (the root stays `/`).
fn file_form(path: &str) -> &str {
    if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    }
}

fn namespace_ok(namespace: &str) -> bool {
    (1..=64).contains(&namespace.len())
        && namespace
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@+:~-".contains(c))
}

/// What storage holds for one file.
struct StoredFile {
    content: String,
    base64: bool,
    size: u64,
}

struct ListedFile {
    path: String,
    size: u64,
    modified: Timestamp,
}

/// What the agent's file tools call (the Agent Loader wraps it as the `files` tool provider).
// spec: ordin-desktop/components/FileService#SA-RECON-05
pub struct WorkFolder {
    store: Datastore,
    namespace: String,
    bundle_dir: PathBuf,
    writable: Option<Vec<String>>,
    protected: Vec<String>,
}

impl WorkFolder {
    /// Checks the namespace, the bundle folder and the writable and protected paths (Internal
    /// Logic 10.1).
    pub fn new(datastore: Datastore, config: WorkFolderConfig) -> Result<WorkFolder, FileError> {
        if !namespace_ok(&config.namespace) {
            return Err(FileError::WorkFolder("WF_INVALID_NAMESPACE"));
        }
        if !config.bundle_dir.is_dir() {
            return Err(FileError::FolderUnreadable);
        }
        let writable = match config.writable_paths {
            None => None,
            Some(list) => {
                let mut out = Vec::with_capacity(list.len());
                for raw in list {
                    let normal = normalise_path(&raw)?;
                    let normal = file_form(&normal).to_owned();
                    if is_bundle_path(&normal) {
                        return Err(invalid_path());
                    }
                    out.push(normal);
                }
                Some(out)
            }
        };
        let mut protected = Vec::with_capacity(config.protected_paths.len());
        for raw in config.protected_paths {
            protected.push(file_form(&normalise_path(&raw)?).to_owned());
        }
        Ok(WorkFolder {
            store: datastore,
            namespace: config.namespace,
            bundle_dir: config.bundle_dir,
            writable,
            protected,
        })
    }

    // ---- storage

    async fn get(&self, path: &str) -> Result<Option<StoredFile>, sqlx::Error> {
        match &self.store {
            Datastore::Sqlite(pool) => {
                let row = sqlx::query(
                    "SELECT content, encoding, size_bytes FROM work_folder_files \
                     WHERE namespace = ?1 AND path = ?2",
                )
                .bind(&self.namespace)
                .bind(path)
                .fetch_optional(pool)
                .await?;
                row.map(|r| stored_from(&r)).transpose()
            }
            Datastore::Postgres(pool) => {
                let row = sqlx::query(
                    "SELECT content, encoding, size_bytes FROM work_folder_files \
                     WHERE namespace = $1 AND path = $2",
                )
                .bind(&self.namespace)
                .bind(path)
                .fetch_optional(pool)
                .await?;
                row.map(|r| stored_from_pg(&r)).transpose()
            }
        }
    }

    async fn put(&self, path: &str, content: &[u8]) -> Result<(), sqlx::Error> {
        let (text, base64) = match std::str::from_utf8(content) {
            Ok(text) => (text.to_owned(), false),
            Err(_) => {
                use base64::Engine as _;
                (
                    base64::engine::general_purpose::STANDARD.encode(content),
                    true,
                )
            }
        };
        let encoding = if base64 { "base64" } else { "utf-8" };
        let size = i64::try_from(content.len()).unwrap_or(i64::MAX);
        let now = Timestamp::now();
        match &self.store {
            Datastore::Sqlite(pool) => {
                sqlx::query(
                    "INSERT INTO work_folder_files \
                         (namespace, path, content, encoding, size_bytes, created_at, modified_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) \
                     ON CONFLICT (namespace, path) DO UPDATE SET content = excluded.content, \
                         encoding = excluded.encoding, size_bytes = excluded.size_bytes, \
                         modified_at = excluded.modified_at",
                )
                .bind(&self.namespace)
                .bind(path)
                .bind(&text)
                .bind(encoding)
                .bind(size)
                .bind(now.to_wire())
                .execute(pool)
                .await?;
            }
            Datastore::Postgres(pool) => {
                sqlx::query(
                    "INSERT INTO work_folder_files \
                         (namespace, path, content, encoding, size_bytes, created_at, modified_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $6) \
                     ON CONFLICT (namespace, path) DO UPDATE SET content = excluded.content, \
                         encoding = excluded.encoding, size_bytes = excluded.size_bytes, \
                         modified_at = excluded.modified_at",
                )
                .bind(&self.namespace)
                .bind(path)
                .bind(&text)
                .bind(encoding)
                .bind(size)
                .bind(now.0)
                .execute(pool)
                .await?;
            }
        }
        Ok(())
    }

    /// Deletes the file at `path` and every file under `path/`; returns how many went.
    async fn delete_tree(&self, path: &str) -> Result<u64, sqlx::Error> {
        let folder = format!("{}/", path.trim_end_matches('/'));
        Ok(match &self.store {
            Datastore::Sqlite(pool) => sqlx::query(
                "DELETE FROM work_folder_files WHERE namespace = ?1 \
                 AND (path = ?2 OR substr(path, 1, length(?3)) = ?3)",
            )
            .bind(&self.namespace)
            .bind(path)
            .bind(&folder)
            .execute(pool)
            .await?
            .rows_affected(),
            Datastore::Postgres(pool) => sqlx::query(
                "DELETE FROM work_folder_files WHERE namespace = $1 \
                 AND (path = $2 OR starts_with(path, $3))",
            )
            .bind(&self.namespace)
            .bind(path)
            .bind(&folder)
            .execute(pool)
            .await?
            .rows_affected(),
        })
    }

    /// Files whose path starts with `prefix`, by path, after `after`, at most `limit`.
    async fn list(
        &self,
        prefix: &str,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ListedFile>, sqlx::Error> {
        let after = after.unwrap_or("");
        match &self.store {
            Datastore::Sqlite(pool) => {
                let rows = sqlx::query(
                    "SELECT path, size_bytes, modified_at FROM work_folder_files \
                     WHERE namespace = ?1 AND substr(path, 1, length(?2)) = ?2 AND path > ?3 \
                     ORDER BY path LIMIT ?4",
                )
                .bind(&self.namespace)
                .bind(prefix)
                .bind(after)
                .bind(limit)
                .fetch_all(pool)
                .await?;
                rows.iter()
                    .map(|r| {
                        let modified: String = r.try_get("modified_at")?;
                        let size: i64 = r.try_get("size_bytes")?;
                        Ok(ListedFile {
                            path: r.try_get("path")?,
                            size: u64::try_from(size).unwrap_or(0),
                            modified: ts(&modified)
                                .map_err(|_| sqlx::Error::Decode("timestamp".into()))?,
                        })
                    })
                    .collect()
            }
            Datastore::Postgres(pool) => {
                let rows = sqlx::query(
                    "SELECT path, size_bytes, modified_at FROM work_folder_files \
                     WHERE namespace = $1 AND starts_with(path, $2) AND path COLLATE \"C\" > $3 \
                     ORDER BY path COLLATE \"C\" LIMIT $4",
                )
                .bind(&self.namespace)
                .bind(prefix)
                .bind(after)
                .bind(limit)
                .fetch_all(pool)
                .await?;
                rows.iter()
                    .map(|r| {
                        let modified: DateTime<Utc> = r.try_get("modified_at")?;
                        let size: i64 = r.try_get("size_bytes")?;
                        Ok(ListedFile {
                            path: r.try_get("path")?,
                            size: u64::try_from(size).unwrap_or(0),
                            modified: Timestamp(modified),
                        })
                    })
                    .collect()
            }
        }
    }

    async fn list_all(&self, prefix: &str) -> Result<Vec<ListedFile>, sqlx::Error> {
        self.list(prefix, None, i64::MAX).await
    }

    fn unreachable(operation: &'static str, error: &sqlx::Error) -> AgentRefusal {
        log::work_folder_unreachable(operation, crate::error::db_class(error));
        AgentRefusal(UNREACHABLE.to_owned())
    }

    fn store_unavailable(operation: &'static str, error: &sqlx::Error) -> FileError {
        log::work_folder_unreachable(operation, crate::error::db_class(error));
        FileError::WorkFolder("WF_STORE_UNAVAILABLE")
    }

    // ---- bundle

    fn bundle_abs(&self, path: &str) -> PathBuf {
        self.bundle_dir.join(path.trim_start_matches('/'))
    }

    async fn bundle_read(&self, path: &str) -> Option<Vec<u8>> {
        let abs = self.bundle_abs(path);
        let bundle_root = self.bundle_dir.clone();
        tokio::task::spawn_blocking(move || {
            let resolved = std::fs::canonicalize(&abs).ok()?;
            let root = std::fs::canonicalize(&bundle_root).ok()?;
            if !resolved.starts_with(root) || !resolved.is_file() {
                return None;
            }
            std::fs::read(resolved).ok()
        })
        .await
        .ok()
        .flatten()
    }

    /// Every bundle file (`/`-rooted path) that exists on local disk.
    async fn bundle_files(&self) -> Vec<(String, PathBuf)> {
        let root = self.bundle_dir.clone();
        tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            for file in WF_BUNDLE_FILES {
                let abs = root.join(file.trim_start_matches('/'));
                if abs.is_file() {
                    out.push((file.to_owned(), abs));
                }
            }
            for prefix in WF_BUNDLE_DIR_PREFIXES {
                let dir = root.join(prefix.trim_matches('/'));
                collect_files(&dir, &format!("/{}", prefix.trim_matches('/')), &mut out);
            }
            out.sort();
            out
        })
        .await
        .unwrap_or_default()
    }

    // ---- agent methods (Internal Logic 10.3–10.5)

    fn path_for_agent(raw: &str) -> Result<String, AgentRefusal> {
        normalise_path(raw)
            .map_err(|_| AgentRefusal(format!("Error: {raw} is not a valid work-folder path")))
    }

    pub async fn ls(&self, path: &str) -> Result<Vec<WorkFileEntry>, AgentRefusal> {
        let normal = Self::path_for_agent(path)?;
        // A file path lists itself.
        if !normal.ends_with('/') && normal != "/" {
            if let Some(entry) = self.entry_for_file(&normal).await? {
                return Ok(vec![entry]);
            }
        }
        let dir = format!("{}/", file_form(&normal));
        let dir = if dir == "//" { "/".to_owned() } else { dir };
        let mut out: Vec<WorkFileEntry> = Vec::new();
        let rows = self
            .list_all(&dir)
            .await
            .map_err(|e| Self::unreachable("ls", &e))?;
        let mut folders: std::collections::BTreeMap<String, Timestamp> = Default::default();
        for row in rows {
            let rest = &row.path[dir.len()..];
            match rest.split_once('/') {
                Some((first, _)) => {
                    let key = format!("{dir}{first}/");
                    let latest = folders.entry(key).or_insert(row.modified);
                    if row.modified > *latest {
                        *latest = row.modified;
                    }
                }
                None => out.push(WorkFileEntry {
                    path: row.path,
                    size_bytes: row.size,
                    modified_at: row.modified,
                    bundle: false,
                }),
            }
        }
        for (path, modified_at) in folders {
            out.push(WorkFileEntry {
                path,
                size_bytes: 0,
                modified_at,
                bundle: false,
            });
        }
        out.extend(self.bundle_children(&dir).await);
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    async fn entry_for_file(&self, path: &str) -> Result<Option<WorkFileEntry>, AgentRefusal> {
        if is_bundle_path(path) {
            let abs = self.bundle_abs(path);
            return Ok(tokio::task::spawn_blocking(move || {
                file_entry(&abs, &abs_path_string(&abs))
            })
            .await
            .ok()
            .flatten()
            .map(|mut e| {
                e.path = path.to_owned();
                e.bundle = true;
                e
            }));
        }
        let rows = self
            .list_all(path)
            .await
            .map_err(|e| Self::unreachable("ls", &e))?;
        Ok(rows
            .into_iter()
            .find(|r| r.path == path)
            .map(|r| WorkFileEntry {
                path: r.path,
                size_bytes: r.size,
                modified_at: r.modified,
                bundle: false,
            }))
    }

    /// The bundle entries directly under `dir` (a `/`-rooted folder ending in `/`).
    async fn bundle_children(&self, dir: &str) -> Vec<WorkFileEntry> {
        let root = self.bundle_dir.clone();
        let dir = dir.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            if dir == "/" {
                for file in WF_BUNDLE_FILES {
                    if let Some(mut e) = file_entry(&root.join(file.trim_start_matches('/')), file)
                    {
                        e.bundle = true;
                        out.push(e);
                    }
                }
                for prefix in WF_BUNDLE_DIR_PREFIXES {
                    let abs = root.join(prefix.trim_matches('/'));
                    if let Some(mut e) = dir_entry(&abs, prefix) {
                        e.bundle = true;
                        out.push(e);
                    }
                }
            } else if is_bundle_path(dir.trim_end_matches('/')) {
                let abs = root.join(dir.trim_matches('/'));
                if let Ok(read) = std::fs::read_dir(&abs) {
                    for entry in read.flatten() {
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        if kind.is_symlink() {
                            continue;
                        }
                        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                            continue;
                        };
                        let path = format!("{dir}{name}");
                        let made = if kind.is_dir() {
                            dir_entry(&entry.path(), &format!("{path}/"))
                        } else {
                            file_entry(&entry.path(), &path)
                        };
                        if let Some(mut e) = made {
                            e.bundle = true;
                            out.push(e);
                        }
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_default()
    }

    /// The text of a file, from the bundle or from storage.
    async fn read_text_of(
        &self,
        normal: &str,
        operation: &'static str,
    ) -> Result<String, AgentRefusal> {
        if normal.ends_with('/') {
            return Err(AgentRefusal(format!("Error: {normal} is a folder")));
        }
        if is_bundle_path(normal) {
            let bytes = self
                .bundle_read(normal)
                .await
                .ok_or_else(|| AgentRefusal(format!("Error: {normal} does not exist")))?;
            return String::from_utf8(bytes)
                .map_err(|_| AgentRefusal(format!("Error: {normal} is not a text file")));
        }
        let stored = self
            .get(normal)
            .await
            .map_err(|e| Self::unreachable(operation, &e))?
            .ok_or_else(|| AgentRefusal(format!("Error: {normal} does not exist")))?;
        if stored.base64 {
            return Err(AgentRefusal(format!("Error: {normal} is not a text file")));
        }
        Ok(stored.content)
    }

    /// `offset` and `limit` count lines.
    pub async fn read(&self, path: &str, offset: u32, limit: u32) -> Result<String, AgentRefusal> {
        let normal = Self::path_for_agent(path)?;
        let text = self.read_text_of(&normal, "read").await?;
        let lines: Vec<&str> = text
            .lines()
            .skip(offset as usize)
            .take(if limit == 0 { 0 } else { limit as usize })
            .collect();
        Ok(lines.join("\n"))
    }

    fn is_writable(&self, normal: &str) -> bool {
        match &self.writable {
            None => true,
            Some(list) => list
                .iter()
                .any(|w| w == "/" || normal == w || normal.starts_with(&format!("{w}/"))),
        }
    }

    /// The write tests of Internal Logic 10.4 that `write`, `edit` and `delete` share; the first
    /// failing test wins.
    fn check_write(&self, normal: &str, size: Option<usize>) -> Result<(), AgentRefusal> {
        if is_bundle_path(file_form(normal)) {
            return Err(AgentRefusal(format!(
                "Error: {normal} is read-only (bundle path)"
            )));
        }
        if !self.is_writable(file_form(normal)) {
            return Err(AgentRefusal(format!(
                "Error: {normal} is outside the writable paths of this agent"
            )));
        }
        if size.is_some_and(|s| s > WF_MAX_FILE_BYTES) {
            return Err(AgentRefusal(format!(
                "Error: {normal} exceeds the limit of {WF_MAX_FILE_BYTES} bytes"
            )));
        }
        Ok(())
    }

    pub async fn write(&self, path: &str, content: &str) -> Result<(), AgentRefusal> {
        let normal = Self::path_for_agent(path)?;
        if normal.ends_with('/') {
            return Err(AgentRefusal(format!("Error: {normal} is a folder")));
        }
        self.check_write(&normal, Some(content.len()))?;
        self.put(&normal, content.as_bytes())
            .await
            .map_err(|e| Self::unreachable("write", &e))
    }

    /// Replaces `old` with `new`; returns how many places changed.
    pub async fn edit(
        &self,
        path: &str,
        old: &str,
        new: &str,
        replace_all: bool,
    ) -> Result<u32, AgentRefusal> {
        let normal = Self::path_for_agent(path)?;
        self.check_write(&normal, None)?;
        let text = self.read_text_of(&normal, "edit").await?;
        if old.is_empty() {
            return Err(AgentRefusal(format!(
                "Error: the text to replace in {normal} is empty"
            )));
        }
        let count = text.matches(old).count();
        if count == 0 {
            return Err(AgentRefusal(format!(
                "Error: the text to replace was not found in {normal}"
            )));
        }
        if count > 1 && !replace_all {
            return Err(AgentRefusal(format!(
                "Error: the text to replace occurs {count} times in {normal}; give more of the surrounding text or set replace_all"
            )));
        }
        let changed = if replace_all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        self.check_write(&normal, Some(changed.len()))?;
        self.put(&normal, changed.as_bytes())
            .await
            .map_err(|e| Self::unreachable("edit", &e))?;
        Ok(u32::try_from(if replace_all { count } else { 1 }).unwrap_or(u32::MAX))
    }

    pub async fn delete(&self, path: &str) -> Result<(), AgentRefusal> {
        let normal = Self::path_for_agent(path)?;
        let target = file_form(&normal).to_owned();
        if target == "/" {
            return Err(AgentRefusal("Error: / cannot be deleted".to_owned()));
        }
        if is_bundle_path(&target) {
            return Err(AgentRefusal(format!(
                "Error: {target} is read-only (bundle path)"
            )));
        }
        if let Some(held) = self
            .protected
            .iter()
            .find(|p| **p == target || p.starts_with(&format!("{target}/")))
        {
            return Err(AgentRefusal(format!(
                "Error: {target} cannot be removed by the agent; a person removes {held} with the operator's remove command"
            )));
        }
        self.check_write(&target, None)?;
        let removed = self
            .delete_tree(&target)
            .await
            .map_err(|e| Self::unreachable("delete", &e))?;
        if removed == 0 {
            return Err(AgentRefusal(format!("Error: {target} does not exist")));
        }
        Ok(())
    }

    /// Shell-style patterns (`*`, `?`, `**`) against the paths under `path` (default `/`).
    pub async fn glob(
        &self,
        pattern: &str,
        path: Option<&str>,
    ) -> Result<Vec<String>, AgentRefusal> {
        let base = match path {
            Some(p) => Self::path_for_agent(p)?,
            None => "/".to_owned(),
        };
        let base = file_form(&base).to_owned();
        let full = if pattern.starts_with('/') {
            pattern.to_owned()
        } else if base == "/" {
            format!("/{pattern}")
        } else {
            format!("{base}/{pattern}")
        };
        let regex = RegexBuilder::new(&glob_to_regex(&full))
            .size_limit(1 << 20)
            .build()
            .map_err(|_| AgentRefusal("Error: the pattern is not valid".to_owned()))?;
        let mut paths: Vec<String> = self
            .all_paths(&base)
            .await?
            .into_iter()
            .filter(|p| regex.is_match(p))
            .collect();
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    /// Every file path under `base`, bundle and work folder.
    async fn all_paths(&self, base: &str) -> Result<Vec<String>, AgentRefusal> {
        let prefix = if base == "/" {
            "/".to_owned()
        } else {
            format!("{base}/")
        };
        let mut out: Vec<String> = self
            .list_all(&prefix)
            .await
            .map_err(|e| Self::unreachable("glob", &e))?
            .into_iter()
            .map(|f| f.path)
            .collect();
        for (path, _) in self.bundle_files().await {
            if path == base || path.starts_with(&prefix) {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// A Rust `regex` (linear time, as ADR-098 item 4), line by line; `max_count` defaults to 100.
    pub async fn grep(
        &self,
        pattern: &str,
        path: Option<&str>,
        max_count: Option<u32>,
    ) -> Result<Vec<GrepMatch>, AgentRefusal> {
        let base = match path {
            Some(p) => Self::path_for_agent(p)?,
            None => "/".to_owned(),
        };
        let base = file_form(&base).to_owned();
        let regex = RegexBuilder::new(pattern)
            .size_limit(1 << 20)
            .build()
            .map_err(|_| {
                AgentRefusal("Error: the pattern is not a valid regular expression".to_owned())
            })?;
        let max = max_count.unwrap_or(DEFAULT_GREP_COUNT) as usize;
        let mut paths = self.all_paths(&base).await?;
        paths.sort();
        let mut out: Vec<GrepMatch> = Vec::new();
        for file in paths {
            if out.len() >= max {
                break;
            }
            // A file that is not text is not searched.
            let Ok(text) = self.read_text_of(&file, "grep").await else {
                continue;
            };
            for (n, line) in text.lines().enumerate() {
                if regex.is_match(line) {
                    out.push(GrepMatch {
                        path: file.clone(),
                        line: u32::try_from(n + 1).unwrap_or(u32::MAX),
                        text: line.to_owned(),
                    });
                    if out.len() >= max {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    // ---- operator routes (Internal Logic 10.7)

    fn operator_path(path: &str) -> Result<String, FileError> {
        Ok(file_form(&normalise_path(path)?).to_owned())
    }

    /// Work-folder files under `prefix`, by path, a page at a time.
    // spec: ordin-desktop/components/FileService#SA-PAGE-01
    pub async fn operator_list(
        &self,
        prefix: &str,
        cursor: Option<Cursor>,
    ) -> Result<Page<WorkFileEntry>, FileError> {
        let normal = normalise_path(prefix)?;
        let folder = if normal == "/" {
            normal
        } else {
            format!("{}/", file_form(&normal))
        };
        if is_bundle_path(file_form(&folder)) {
            return Err(FileError::WorkFolder("WF_BUNDLE_PATH"));
        }
        let after = match cursor {
            Some(cursor) => Some(cursor.decode::<String>()?),
            None => None,
        };
        let mut rows = self
            .list(&folder, after.as_deref(), PAGE_SIZE as i64 + 1)
            .await
            .map_err(|e| Self::store_unavailable("operator_list", &e))?;
        let next_cursor = if rows.len() > PAGE_SIZE {
            rows.truncate(PAGE_SIZE);
            rows.last().map(|r| Cursor::encode(&r.path))
        } else {
            None
        };
        Ok(Page {
            items: rows
                .into_iter()
                .map(|r| WorkFileEntry {
                    path: r.path,
                    size_bytes: r.size,
                    modified_at: r.modified,
                    bundle: false,
                })
                .collect(),
            next_cursor,
        })
    }

    // spec: ordin-desktop/components/FileService#SA-RECON-06
    pub async fn operator_read(&self, path: &str) -> Result<Vec<u8>, FileError> {
        let normal = Self::operator_path(path)?;
        if is_bundle_path(&normal) {
            return Err(FileError::WorkFolder("WF_BUNDLE_PATH"));
        }
        let stored = self
            .get(&normal)
            .await
            .map_err(|e| Self::store_unavailable("operator_read", &e))?
            .ok_or(FileError::WorkFolder("WF_NOT_FOUND"))?;
        if stored.base64 {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(stored.content)
                .map_err(|_| FileError::WorkFolder("WF_STORE_UNAVAILABLE"))
        } else {
            let _ = stored.size;
            Ok(stored.content.into_bytes())
        }
    }

    // spec: ordin-desktop/components/FileService#SA-RECON-06
    pub async fn operator_write(&self, path: &str, content: Vec<u8>) -> Result<(), FileError> {
        let normal = Self::operator_path(path)?;
        if is_bundle_path(&normal) || normal == "/" {
            return Err(FileError::WorkFolder("WF_BUNDLE_PATH"));
        }
        if content.len() > WF_MAX_FILE_BYTES {
            return Err(FileError::WorkFolder("WF_TOO_LARGE"));
        }
        self.put(&normal, &content)
            .await
            .map_err(|e| Self::store_unavailable("operator_write", &e))
    }

    /// Removes the file, or every file under the folder, with no protected-path test.
    // spec: ordin-desktop/components/FileService#SA-RECON-06
    pub async fn operator_remove(&self, path: &str) -> Result<(), FileError> {
        let normal = Self::operator_path(path)?;
        if is_bundle_path(&normal) {
            return Err(FileError::WorkFolder("WF_BUNDLE_PATH"));
        }
        let removed = self
            .delete_tree(&normal)
            .await
            .map_err(|e| Self::store_unavailable("operator_remove", &e))?;
        if removed == 0 {
            return Err(FileError::WorkFolder("WF_NOT_FOUND"));
        }
        Ok(())
    }

    /// True when a work-folder file exists at the path (bundle paths → `WF_BUNDLE_PATH`).
    // spec: ordin-desktop/components/FileService#SA-RECON-06
    pub async fn operator_exists(&self, path: &str) -> Result<bool, FileError> {
        let normal = Self::operator_path(path)?;
        if is_bundle_path(&normal) {
            return Err(FileError::WorkFolder("WF_BUNDLE_PATH"));
        }
        Ok(self
            .get(&normal)
            .await
            .map_err(|e| Self::store_unavailable("operator_exists", &e))?
            .is_some())
    }
}

fn stored_from(row: &sqlx::sqlite::SqliteRow) -> Result<StoredFile, sqlx::Error> {
    let encoding: String = row.try_get("encoding")?;
    let size: i64 = row.try_get("size_bytes")?;
    Ok(StoredFile {
        content: row.try_get("content")?,
        base64: encoding == "base64",
        size: u64::try_from(size).unwrap_or(0),
    })
}

fn stored_from_pg(row: &sqlx::postgres::PgRow) -> Result<StoredFile, sqlx::Error> {
    let encoding: String = row.try_get("encoding")?;
    let size: i64 = row.try_get("size_bytes")?;
    Ok(StoredFile {
        content: row.try_get("content")?,
        base64: encoding == "base64",
        size: u64::try_from(size).unwrap_or(0),
    })
}

/// `*` (not `/`), `?` (one character, not `/`), `**` (anything, and `**/` may match nothing).
fn glob_to_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    out.push_str(".*");
                    i += 2;
                }
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            other => out.push_str(&regex::escape(&other.to_string())),
        }
        i += 1;
    }
    out.push('$');
    out
}

fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let path = format!("{prefix}/{name}");
        if kind.is_dir() {
            collect_files(&entry.path(), &path, out);
        } else {
            out.push((path, entry.path()));
        }
    }
}

fn modified_of(meta: &std::fs::Metadata) -> Timestamp {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| Timestamp::from_unix_millis(i64::try_from(d.as_millis()).ok()?).ok())
        .unwrap_or_else(Timestamp::now)
}

fn file_entry(abs: &Path, path: &str) -> Option<WorkFileEntry> {
    let meta = std::fs::metadata(abs).ok().filter(|m| m.is_file())?;
    Some(WorkFileEntry {
        path: path.to_owned(),
        size_bytes: meta.len(),
        modified_at: modified_of(&meta),
        bundle: false,
    })
}

fn dir_entry(abs: &Path, path: &str) -> Option<WorkFileEntry> {
    let meta = std::fs::metadata(abs).ok().filter(|m| m.is_dir())?;
    Some(WorkFileEntry {
        path: path.to_owned(),
        size_bytes: 0,
        modified_at: modified_of(&meta),
        bundle: false,
    })
}

fn abs_path_string(abs: &Path) -> String {
    abs.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_normalised() {
        assert_eq!(normalise_path("/a//b/./c").unwrap(), "/a/b/c");
        assert_eq!(normalise_path("/a/b/").unwrap(), "/a/b/");
        assert_eq!(normalise_path("/").unwrap(), "/");
        for bad in [
            "a/b",
            "/a/../b",
            "/a\0b",
            "/~x",
            &format!("/{}", "a".repeat(1024)),
        ] {
            assert_eq!(
                normalise_path(bad).unwrap_err().code(),
                "WF_INVALID_PATH",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn bundle_paths() {
        for yes in [
            "/AGENTS.md",
            "/.apm/x",
            "/knowledge",
            "/knowledge/a.md",
            "/brand/logo",
        ] {
            assert!(is_bundle_path(yes), "{yes}");
        }
        for no in ["/plans/a.md", "/knowledgebase", "/agents.md", "/"] {
            assert!(!is_bundle_path(no), "{no}");
        }
    }

    #[test]
    fn globs() {
        let re = |p: &str| regex::Regex::new(&glob_to_regex(p)).unwrap();
        assert!(re("/plans/*.md").is_match("/plans/a.md"));
        assert!(!re("/plans/*.md").is_match("/plans/x/a.md"));
        assert!(re("/**/a.md").is_match("/a.md"));
        assert!(re("/**/a.md").is_match("/x/y/a.md"));
        assert!(re("/p?.md").is_match("/pa.md"));
        assert!(re("/a.b").is_match("/a.b") && !re("/a.b").is_match("/axb"));
        assert!(re("/x/**").is_match("/x/y/z"));
    }

    #[test]
    fn namespaces() {
        assert!(namespace_ok("watch-readiness") && namespace_ok("a@b+c:d~e_f"));
        assert!(!namespace_ok("") && !namespace_ok("has space") && !namespace_ok(&"a".repeat(65)));
    }
}
