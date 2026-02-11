//! The program's log files (OrdinDesktop.md *Internal Logic* step 2 and DSK-LOG-1 to DSK-LOG-4;
//! cross-cutting/Logging.md L-1, both amended January 2026).
//!
//! `tracing-appender` 0.2 starts a new file by time only, so the program keeps its own files and
//! hands this writer to `tracing_appender::non_blocking`, which still does the writing on its own
//! thread. Files are `ordin.<UTC date>.log`, then `ordin.<UTC date>.<n>.log` (n = 1, 2, …) when a
//! file of the same day reaches its size limit. At most 14 files are kept and the folder holds at
//! most 100 MB; the oldest go first, checked at start and each time a new file starts.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use ordin_service_api::commands::LogsCleared;

use crate::error::{LOGS_CLEAR_FAILED, LOG_FILE_NOT_DELETED, SUPPORT_LOG_UNREADABLE};

/// The first part of every log file's name.
const PREFIX: &str = "ordin";
/// The extension of every log file.
const SUFFIX: &str = "log";

/// How many files, how large one file and how large the folder may grow (DSK-LOG-1, DSK-LOG-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Files kept, the current one included.
    pub max_files: usize,
    /// A new file starts before a line would take the current one past this many bytes.
    pub max_file_bytes: u64,
    /// The folder's log files together, the current one included.
    pub max_folder_bytes: u64,
}

impl Limits {
    /// The desktop's limits: 14 files, 20 MB a file, 100 MB in all (1 MB = 1,000,000 bytes).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-1
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-2
    pub const DESKTOP: Limits = Limits {
        max_files: 14,
        max_file_bytes: 20_000_000,
        max_folder_bytes: 100_000_000,
    };
}

/// A log file's place in time: its UTC day (`YYYY-MM-DD`), then its number within the day, 0 for
/// the day's first file. The derived order is oldest first.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    day: String,
    part: u32,
}

impl Key {
    fn file_name(&self) -> String {
        if self.part == 0 {
            format!("{PREFIX}.{}.{SUFFIX}", self.day)
        } else {
            format!("{PREFIX}.{}.{}.{SUFFIX}", self.day, self.part)
        }
    }

    /// The key of a log file's name; `None` for any other file in the folder.
    fn parse(name: &str) -> Option<Key> {
        let rest = name
            .strip_prefix(PREFIX)?
            .strip_prefix('.')?
            .strip_suffix(SUFFIX)?
            .strip_suffix('.')?;
        let (day, part) = match rest.split_once('.') {
            None => (rest, 0),
            Some((day, part)) => (day, part.parse::<u32>().ok().filter(|n| *n > 0)?),
        };
        let bytes = day.as_bytes();
        let is_day = bytes.len() == 10
            && bytes.iter().enumerate().all(|(i, b)| match i {
                4 | 7 => *b == b'-',
                _ => b.is_ascii_digit(),
            });
        is_day.then(|| Key {
            day: day.to_owned(),
            part,
        })
    }
}

/// The UTC day of now, as the file names carry it.
fn utc_day() -> String {
    ordin_types::time::Timestamp::now()
        .0
        .format("%Y-%m-%d")
        .to_string()
}

type Clock = Box<dyn Fn() -> String + Send>;

/// One entry of the folder whose name is a log file's: its key, path, size, and whether it is a
/// file. Anything else under such a name (a folder) is listed too, so that it counts as a file that
/// cannot be deleted rather than being skipped silently.
struct Listed {
    key: Key,
    path: PathBuf,
    len: u64,
    is_file: bool,
}

/// The log files in `dir`, oldest first.
fn list(dir: &Path) -> io::Result<Vec<Listed>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(key) = entry.file_name().to_str().and_then(Key::parse) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        files.push(Listed {
            key,
            path: entry.path(),
            len: if meta.is_file() { meta.len() } else { 0 },
            is_file: meta.is_file(),
        });
    }
    files.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(files)
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

struct State {
    dir: PathBuf,
    limits: Limits,
    clock: Clock,
    key: Key,
    file: File,
    /// Bytes in the current file.
    written: u64,
}

impl State {
    /// Starts writing to the file of `key`, continuing it when it exists.
    fn start(&mut self, key: Key) -> io::Result<()> {
        let path = self.dir.join(key.file_name());
        let file = open_append(&path)?;
        self.written = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = file;
        self.key = key;
        Ok(())
    }

    /// The next number of the current day: one above the highest in the folder.
    fn next_part(&self) -> u32 {
        let highest = list(&self.dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.key.day == self.key.day)
            .map(|f| f.key.part)
            .max()
            .unwrap_or(0);
        highest.max(self.key.part) + 1
    }

    /// Deletes the oldest files other than the current one while more than `max_files` are kept or
    /// the folder holds more than `max_folder_bytes`; answers the names it could not delete. Such a
    /// file stays, and the next check tries again.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-2
    fn prune(&mut self) -> Vec<String> {
        let Ok(files) = list(&self.dir) else {
            return Vec::new();
        };
        let older: Vec<_> = files.into_iter().filter(|f| f.key != self.key).collect();
        let mut count = older.len() + 1;
        let mut total = older.iter().map(|f| f.len).sum::<u64>() + self.written;
        let mut not_deleted = Vec::new();
        for file in older {
            if count <= self.limits.max_files && total <= self.limits.max_folder_bytes {
                break;
            }
            match fs::remove_file(&file.path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => {
                    not_deleted.push(file.key.file_name());
                    continue;
                }
            }
            count -= 1;
            total = total.saturating_sub(file.len);
        }
        not_deleted
    }

    /// Writes `buf` whole to the current file, first starting a new one when the day has changed or
    /// when `buf` would take a file that already holds bytes past its size limit.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-1
    fn write(&mut self, buf: &[u8]) -> io::Result<Vec<String>> {
        let day = (self.clock)();
        let mut not_deleted = Vec::new();
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if day != self.key.day {
            self.start(Key { day, part: 0 })?;
            not_deleted = self.prune();
        } else if self.written > 0 && self.written.saturating_add(len) > self.limits.max_file_bytes
        {
            let part = self.next_part();
            self.start(Key {
                day: self.key.day.clone(),
                part,
            })?;
            not_deleted = self.prune();
        }
        self.file.write_all(buf)?;
        self.written = self.written.saturating_add(len);
        Ok(not_deleted)
    }
}

/// Why **Clear logs** did not finish (DSK-LOG-4); either way logging carries on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearError {
    /// The folder could not be read, so nothing was deleted.
    FolderUnreadable,
    /// These past files could not be deleted; every other one was.
    NotDeleted(Vec<String>),
    /// The current file could not be emptied; every past file was deleted.
    NotEmptied,
}

/// The program's log files: a `Write` for the log's writer thread, and **Clear logs**.
#[derive(Clone)]
pub struct LogFiles {
    state: Arc<Mutex<State>>,
}

impl LogFiles {
    /// Opens the folder at start: continues today's newest file while it is under the size limit,
    /// else starts the next one, then deletes the oldest files over the limits (DSK-LOG-2). Answers
    /// the names of files it could not delete, for the log once it is installed.
    pub fn open(dir: &Path, limits: Limits) -> io::Result<(LogFiles, Vec<String>)> {
        Self::open_with_clock(dir, limits, Box::new(utc_day))
    }

    fn open_with_clock(
        dir: &Path,
        limits: Limits,
        clock: Clock,
    ) -> io::Result<(LogFiles, Vec<String>)> {
        fs::create_dir_all(dir)?;
        let day = clock();
        let today: Vec<Listed> = list(dir)?
            .into_iter()
            .filter(|f| f.key.day == day)
            .collect();
        let highest = today.iter().map(|f| f.key.part).max();
        let mut key = Key { day, part: 0 };
        if let Some(newest) = today.into_iter().last() {
            // Continued only when it is a file under the size limit; else the day's next number.
            key = newest.key;
            if !newest.is_file || newest.len >= limits.max_file_bytes {
                key.part = highest.unwrap_or(0) + 1;
            }
        }
        let path = dir.join(key.file_name());
        let file = open_append(&path)?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut state = State {
            dir: dir.to_owned(),
            limits,
            clock,
            key,
            file,
            written,
        };
        let not_deleted = state.prune();
        Ok((
            LogFiles {
                state: Arc::new(Mutex::new(state)),
            },
            not_deleted,
        ))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The name of the file being written now.
    pub fn current_file_name(&self) -> String {
        self.lock().key.file_name()
    }

    /// **Clear logs** (DSK-LOG-4): deletes every log file but the current one, then empties the
    /// current one; the writer keeps its place, so logging never stops. Answers how many files were
    /// deleted.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-4
    pub fn clear(&self) -> Result<u32, ClearError> {
        let mut state = self.lock();
        let files = list(&state.dir).map_err(|_| ClearError::FolderUnreadable)?;
        let mut deleted = 0u32;
        let mut not_deleted = Vec::new();
        for file in files {
            if file.key == state.key {
                continue;
            }
            match fs::remove_file(&file.path) {
                Ok(()) => deleted += 1,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => not_deleted.push(file.key.file_name()),
            }
        }
        // A new append handle after truncating: on Windows an append handle may not set the length.
        let path = state.dir.join(state.key.file_name());
        let emptied = File::create(&path).and_then(|_| open_append(&path));
        let emptied = match emptied {
            Ok(file) => {
                state.file = file;
                state.written = 0;
                true
            }
            Err(_) => false,
        };
        if !not_deleted.is_empty() {
            return Err(ClearError::NotDeleted(not_deleted));
        }
        if !emptied {
            return Err(ClearError::NotEmptied);
        }
        Ok(deleted)
    }
}

impl Write for LogFiles {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let not_deleted = self.lock().write(buf)?;
        // Logged after the lock is released: the line comes back through this writer.
        for file in not_deleted {
            tracing::warn!(
                component = "desktop",
                code = LOG_FILE_NOT_DELETED,
                file = file.as_str(),
                "log_file_not_deleted"
            );
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.lock().file.flush()
    }
}

/// The log part of the support file holds at most this many bytes (DSK-LOG-5).
pub const SUPPORT_LOG_BYTES: usize = 40_000_000;
/// The days whose files go into the support file: the current UTC day and the two before it.
const SUPPORT_DAYS: i64 = 3;
const DAY_MS: i64 = 86_400_000;

/// The log part of the support file (DSK-LOG-5): JSON Lines to append to the Journal's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportLogs {
    pub bytes: Vec<u8>,
    /// Files read.
    pub files: u32,
    /// Files of those days that could not be read.
    pub unreadable: Vec<String>,
}

/// `day` and the days before it, `count` in all, newest first; only `day` when it is not a date.
fn days_back(day: &str, count: i64) -> Vec<String> {
    let Ok(start) = ordin_types::time::parse_rfc3339_ms_utc(&format!("{day}T00:00:00.000Z")) else {
        return vec![day.to_owned()];
    };
    let start = ordin_types::time::Timestamp(start).unix_millis();
    (0..count)
        .filter_map(|back| {
            ordin_types::time::Timestamp::from_unix_millis(start - back * DAY_MS)
                .ok()
                .map(|t| t.0.format("%Y-%m-%d").to_string())
        })
        .collect()
}

fn record(value: serde_json::Value) -> Vec<u8> {
    let mut line = serde_json::to_vec(&value).unwrap_or_default();
    line.push(b'\n');
    line
}

/// One log file's whole lines: the current file only up to `limit` bytes and its last line break.
fn read_lines(path: &Path, limit: Option<u64>) -> io::Result<Vec<String>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    match limit {
        Some(limit) => {
            File::open(path)?.take(limit).read_to_end(&mut bytes)?;
            // A line still being written, or one after **Clear logs** emptied the file, is left out.
            let whole = bytes
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |at| at + 1);
            bytes.truncate(whole);
        }
        None => {
            File::open(path)?.read_to_end(&mut bytes)?;
        }
    }
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

impl LogFiles {
    /// DSK-LOG-5: the lines of the log files of the current UTC day and the two before it, oldest
    /// first, each a `log_line` record, the newest kept within `max_bytes`. Under the lock the current
    /// file is flushed and its length taken; the files are read after the lock is released.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-5
    pub fn support_logs(&self, max_bytes: usize) -> SupportLogs {
        self.support_logs_since(max_bytes, None)
    }

    /// DSK-LOG-6: as `support_logs`, keeping only lines whose `time` is at or after `since` (the same
    /// fixed-width UTC form, so text order is time order); a line with no `time` is kept. A
    /// `log_window` record names the window first.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-6
    pub fn support_logs_since(&self, max_bytes: usize, since: Option<&str>) -> SupportLogs {
        let (dir, current, length, today) = {
            let mut state = self.lock();
            let _ = state.file.flush();
            let today = (state.clock)();
            (state.dir.clone(), state.key.clone(), state.written, today)
        };
        let days = days_back(&today, SUPPORT_DAYS);
        let files: Vec<Listed> = list(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| days.contains(&f.key.day))
            .collect();
        let mut lines: Vec<Vec<u8>> = Vec::new();
        let mut unreadable = Vec::new();
        let mut read = 0u32;
        for file in &files {
            let name = file.key.file_name();
            let limit = (file.key == current).then_some(length);
            let found = if file.is_file {
                read_lines(&file.path, limit)
            } else {
                Err(io::Error::other("not a file"))
            };
            match found {
                Ok(found) => {
                    read += 1;
                    let recent = |line: &String| match since {
                        None => true,
                        Some(since) => line_time(line).is_none_or(|time| time.as_str() >= since),
                    };
                    lines.extend(found.into_iter().filter(recent).map(|line| {
                        record(serde_json::json!({
                            "record": "log_line",
                            "file": name,
                            "line": line,
                        }))
                    }));
                }
                Err(_) => {
                    tracing::warn!(
                        component = "desktop",
                        code = SUPPORT_LOG_UNREADABLE,
                        file = name.as_str(),
                        "support_log_file_unreadable"
                    );
                    unreadable.push(name);
                }
            }
        }
        let named: Vec<u8> = unreadable
            .iter()
            .flat_map(|name| {
                record(serde_json::json!({
                    "record": "log_file_unreadable",
                    "file": name,
                }))
            })
            .collect();
        // The newest lines are kept: count back from the last line while they fit beside the
        // records naming unreadable files and room for the `log_lines_cut` record.
        let budget = max_bytes.saturating_sub(named.len() + 64);
        let mut total = 0usize;
        let mut first_kept = lines.len();
        while first_kept > 0 && total + lines[first_kept - 1].len() <= budget {
            total += lines[first_kept - 1].len();
            first_kept -= 1;
        }
        let mut bytes = Vec::with_capacity(total);
        if let Some(since) = since {
            bytes.extend(record(serde_json::json!({
                "record": "log_window",
                "since": since,
            })));
        }
        if first_kept > 0 {
            bytes.extend(record(serde_json::json!({
                "record": "log_lines_cut",
                "left_out": first_kept,
            })));
        }
        for line in &lines[first_kept..] {
            bytes.extend_from_slice(line);
        }
        bytes.extend(named);
        SupportLogs {
            bytes,
            files: read,
            unreadable,
        }
    }
}

/// The `time` field of one log line, when it has one.
fn line_time(line: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("time")?
        .as_str()
        .map(str::to_owned)
}

/// DSK-LOG-5 over the installed log files; with no log open, the one record `logs_not_open`.
/// DSK-LOG-6: `since` keeps only the lines of the window.
// spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-5
pub fn support_logs(files: Option<&LogFiles>, since: Option<&str>) -> SupportLogs {
    match files {
        Some(files) => files.support_logs_since(SUPPORT_LOG_BYTES, since),
        None => SupportLogs {
            bytes: record(serde_json::json!({ "record": "logs_not_open" })),
            files: 0,
            unreadable: Vec::new(),
        },
    }
}

/// Step 2's subscriber over the program's log files: ordin's one line form
/// (`ordin_types::log::JsonLineFormat`, so rules R-2 to R-7 apply to every line the support file
/// later copies), level `info` for ordin's crates and `warn` for every other crate, written through
/// `tracing_appender::non_blocking`. The program holds the guard for as long as it runs.
// spec: ordin-desktop/cross-cutting/Logging#L-1
// spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-1
pub fn subscriber(
    files: LogFiles,
) -> (
    impl tracing::Subscriber + Send + Sync + 'static,
    tracing_appender::non_blocking::WorkerGuard,
) {
    let (writer, guard) = tracing_appender::non_blocking(files);
    (
        ordin_types::log::subscriber(writer, tracing::Level::INFO),
        guard,
    )
}

/// The log files of this process, set by step 2 once the log is installed.
static INSTALLED: RwLock<Option<LogFiles>> = RwLock::new(None);

/// Records the log files step 2 installed, for `diagnostics.clear_logs`.
pub fn set_installed(files: Option<LogFiles>) {
    *INSTALLED.write().unwrap_or_else(PoisonError::into_inner) = files;
}

/// The log files step 2 installed; `None` when no log could be opened.
pub fn installed() -> Option<LogFiles> {
    INSTALLED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// `diagnostics.clear_logs` over `files` (DSK-LOG-4): the count of deleted files, else
/// `DSK_LOGS_CLEAR_FAILED`. No log open at all is the same refusal: there is nothing it could clear.
// spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-4
pub fn clear_logs(files: Option<&LogFiles>) -> Result<LogsCleared, &'static str> {
    let Some(files) = files else {
        tracing::warn!(
            component = "desktop",
            code = LOGS_CLEAR_FAILED,
            "logs_clear_failed"
        );
        return Err(LOGS_CLEAR_FAILED);
    };
    match files.clear() {
        Ok(deleted_files) => {
            tracing::info!(component = "desktop", deleted_files, "logs_cleared");
            Ok(LogsCleared { deleted_files })
        }
        Err(error) => {
            let not_deleted = match &error {
                ClearError::NotDeleted(names) => names.len(),
                _ => 0,
            };
            tracing::warn!(
                component = "desktop",
                code = LOGS_CLEAR_FAILED,
                not_deleted,
                emptied = !matches!(error, ClearError::NotEmptied),
                "logs_clear_failed"
            );
            Err(LOGS_CLEAR_FAILED)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    /// A clock the test moves by hand.
    fn clock(day: &str) -> (Arc<Mutex<String>>, Clock) {
        let now = Arc::new(Mutex::new(day.to_owned()));
        let read = Arc::clone(&now);
        (now, Box::new(move || read.lock().expect("clock").clone()))
    }

    fn names(dir: &Path) -> Vec<String> {
        list(dir)
            .expect("list")
            .into_iter()
            .map(|f| f.key.file_name())
            .collect()
    }

    fn put(dir: &Path, name: &str, len: usize) {
        fs::write(dir.join(name), vec![b'x'; len]).expect("write");
    }

    const SMALL: Limits = Limits {
        max_files: 14,
        max_file_bytes: 100,
        max_folder_bytes: 10_000,
    };

    #[test]
    fn names_are_read_back_in_age_order_and_other_files_are_ignored() {
        assert_eq!(
            Key::parse("ordin.2026-11-18.log"),
            Some(Key {
                day: "2026-11-18".into(),
                part: 0
            })
        );
        assert_eq!(
            Key::parse("ordin.2026-11-18.3.log").map(|k| k.part),
            Some(3)
        );
        for other in [
            "ordin.2026-11-18.0.log",
            "ordin.2026-11-18.x.log",
            "ordin.2026-1-18.log",
            "other.2026-11-18.log",
            "ordin.2026-11-18.txt",
            "journal.db",
        ] {
            assert_eq!(Key::parse(other), None, "{other}");
        }
        let dir = TempDir::new("logs-order");
        for name in [
            "ordin.2026-11-18.2.log",
            "ordin.2026-11-17.log",
            "ordin.2026-11-18.log",
            "ordin.2026-11-18.10.log",
            "notes.txt",
        ] {
            put(dir.path(), name, 1);
        }
        assert_eq!(
            names(dir.path()),
            [
                "ordin.2026-11-17.log",
                "ordin.2026-11-18.log",
                "ordin.2026-11-18.2.log",
                "ordin.2026-11-18.10.log"
            ]
        );
    }

    // Scenario: a new file starts each UTC day (DSK-LOG-1).
    #[test]
    fn a_new_file_starts_each_day() {
        let dir = TempDir::new("logs-daily");
        let (now, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        files.write_all(b"one\n").expect("write");
        *now.lock().expect("clock") = "2026-11-19".into();
        files.write_all(b"two\n").expect("write");
        assert_eq!(files.current_file_name(), "ordin.2026-11-19.log");
        assert_eq!(
            fs::read(dir.path().join("ordin.2026-11-18.log")).expect("read"),
            b"one\n"
        );
        assert_eq!(
            fs::read(dir.path().join("ordin.2026-11-19.log")).expect("read"),
            b"two\n"
        );
    }

    // Scenario: a file that reaches 20 MB is followed by a new one the same day (DSK-LOG-1).
    #[test]
    fn a_new_file_starts_within_the_day_at_the_size_limit_and_no_line_is_split() {
        let dir = TempDir::new("logs-size");
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        let line = [b'a'; 40];
        for _ in 0..5 {
            files.write_all(&line).expect("write");
        }
        // 40 + 40 = 80 fits in 100; a third line would be 120.
        assert_eq!(
            names(dir.path()),
            [
                "ordin.2026-11-18.log",
                "ordin.2026-11-18.1.log",
                "ordin.2026-11-18.2.log"
            ]
        );
        for name in names(dir.path()) {
            let len = fs::metadata(dir.path().join(&name)).expect("meta").len();
            assert!(len <= 100 && len.is_multiple_of(40), "{name}: {len}");
        }
        // One line larger than the limit is written whole to a file of its own.
        files.write_all(&[b'b'; 150]).expect("write");
        assert_eq!(files.current_file_name(), "ordin.2026-11-18.3.log");
        assert_eq!(
            fs::metadata(dir.path().join("ordin.2026-11-18.3.log"))
                .expect("meta")
                .len(),
            150
        );
    }

    #[test]
    fn at_start_the_day_s_newest_file_is_continued_until_it_is_full() {
        let dir = TempDir::new("logs-continue");
        put(dir.path(), "ordin.2026-11-18.log", 100);
        put(dir.path(), "ordin.2026-11-18.1.log", 30);
        let (_, clock_a) = clock("2026-11-18");
        let (files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock_a).expect("open");
        assert_eq!(files.current_file_name(), "ordin.2026-11-18.1.log");
        drop(files);
        put(dir.path(), "ordin.2026-11-18.1.log", 100);
        let (_, clock_b) = clock("2026-11-18");
        let (files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock_b).expect("open");
        assert_eq!(files.current_file_name(), "ordin.2026-11-18.2.log");
    }

    // Scenario: at most 14 files are kept, the oldest deleted first (DSK-LOG-2).
    #[test]
    fn at_most_fourteen_files_are_kept_oldest_deleted_first_at_start_and_at_each_new_file() {
        let dir = TempDir::new("logs-count");
        for day in 1..=15 {
            put(dir.path(), &format!("ordin.2026-10-{day:02}.log"), 1);
        }
        let (now, clock) = clock("2026-11-18");
        let (mut files, not_deleted) =
            LogFiles::open_with_clock(dir.path(), Limits::DESKTOP, clock).expect("open");
        assert!(not_deleted.is_empty());
        let kept = names(dir.path());
        assert_eq!(kept.len(), 14);
        assert_eq!(
            kept.first().map(String::as_str),
            Some("ordin.2026-10-03.log")
        );
        assert_eq!(
            kept.last().map(String::as_str),
            Some("ordin.2026-11-18.log")
        );
        *now.lock().expect("clock") = "2026-11-19".into();
        files.write_all(b"x\n").expect("write");
        let kept = names(dir.path());
        assert_eq!(kept.len(), 14);
        assert_eq!(
            kept.first().map(String::as_str),
            Some("ordin.2026-10-04.log")
        );
    }

    // Scenario: the folder holds at most 100 MB, the oldest deleted first (DSK-LOG-2).
    #[test]
    fn the_folder_stays_under_its_byte_limit_oldest_deleted_first() {
        let limits = Limits {
            max_files: 14,
            max_file_bytes: 100,
            max_folder_bytes: 250,
        };
        let dir = TempDir::new("logs-bytes");
        put(dir.path(), "ordin.2026-11-15.log", 100);
        put(dir.path(), "ordin.2026-11-16.log", 100);
        put(dir.path(), "ordin.2026-11-17.log", 100);
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), limits, clock).expect("open");
        // At start: 300 bytes, so the oldest goes.
        assert_eq!(
            names(dir.path()),
            [
                "ordin.2026-11-16.log",
                "ordin.2026-11-17.log",
                "ordin.2026-11-18.log"
            ]
        );
        files.write_all(&[b'a'; 60]).expect("write");
        // A new file starts at 60 + 60 > 100: 100 + 100 + 60 + 60 = 320 > 250.
        files.write_all(&[b'a'; 60]).expect("write");
        assert_eq!(
            names(dir.path()),
            [
                "ordin.2026-11-17.log",
                "ordin.2026-11-18.log",
                "ordin.2026-11-18.1.log"
            ]
        );
    }

    // Scenario: Clear logs deletes every past file and empties the current one; logging carries on
    // (DSK-LOG-4).
    #[test]
    fn clear_deletes_past_files_empties_the_current_one_and_logging_carries_on() {
        let dir = TempDir::new("logs-clear");
        put(dir.path(), "ordin.2026-11-16.log", 10);
        put(dir.path(), "ordin.2026-11-17.log", 10);
        put(dir.path(), "ordin.2026-11-18.log", 10);
        put(dir.path(), "notes.txt", 3);
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        files.write_all(b"before\n").expect("write");
        assert_eq!(files.clear(), Ok(2));
        assert_eq!(names(dir.path()), ["ordin.2026-11-18.log"]);
        assert!(dir.path().join("notes.txt").exists(), "only log files go");
        assert_eq!(
            fs::metadata(dir.path().join("ordin.2026-11-18.log"))
                .expect("meta")
                .len(),
            0
        );
        files.write_all(b"after\n").expect("write");
        files.flush().expect("flush");
        assert_eq!(
            fs::read(dir.path().join("ordin.2026-11-18.log")).expect("read"),
            b"after\n"
        );
        // The size count starts again from the emptied file.
        files.write_all(&[b'a'; 90]).expect("write");
        assert_eq!(files.current_file_name(), "ordin.2026-11-18.log");
    }

    /// A folder under a log file's name: `remove_file` refuses it on every system the program ships
    /// for, so it stands in for a log file the system will not delete.
    fn undeletable(dir: &Path, name: &str) {
        fs::create_dir_all(dir.join(name).join("inside")).expect("dir");
    }

    // Error path: a past file that cannot be deleted is DSK_LOGS_CLEAR_FAILED; every other past file
    // goes and the current one is still emptied (ErrorHandling.md, DSK_LOGS_CLEAR_FAILED).
    #[test]
    fn a_file_that_cannot_be_deleted_fails_the_clear_and_everything_else_is_cleared() {
        let dir = TempDir::new("logs-clear-fail");
        undeletable(dir.path(), "ordin.2026-11-16.log");
        put(dir.path(), "ordin.2026-11-17.log", 10);
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        files.write_all(b"before\n").expect("write");
        assert_eq!(
            files.clear(),
            Err(ClearError::NotDeleted(vec!["ordin.2026-11-16.log".into()]))
        );
        assert!(!dir.path().join("ordin.2026-11-17.log").exists());
        assert_eq!(
            fs::metadata(dir.path().join("ordin.2026-11-18.log"))
                .expect("meta")
                .len(),
            0
        );
        assert_eq!(clear_logs(Some(&files)), Err(LOGS_CLEAR_FAILED));
        // Logging carries on.
        files.write_all(b"after\n").expect("write");
        assert_eq!(
            fs::read(dir.path().join("ordin.2026-11-18.log")).expect("read"),
            b"after\n"
        );
    }

    // Error path: no log open is DSK_LOGS_CLEAR_FAILED; an open one answers the count it deleted.
    #[test]
    fn the_command_answers_the_count_or_the_refusal() {
        assert_eq!(clear_logs(None), Err(LOGS_CLEAR_FAILED));
        let dir = TempDir::new("logs-command");
        put(dir.path(), "ordin.2026-11-17.log", 10);
        let (_, clock) = clock("2026-11-18");
        let (files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        assert_eq!(
            clear_logs(Some(&files)),
            Ok(LogsCleared { deleted_files: 1 })
        );
    }

    // Error path: a file the limits would delete but cannot is answered by name, stays, and is tried
    // again at the next new file (DSK_LOG_FILE_NOT_DELETED); the newer files are still kept to the
    // limit.
    #[test]
    fn a_file_the_limits_cannot_delete_is_reported_and_tried_again() {
        let limits = Limits {
            max_files: 2,
            max_file_bytes: 100,
            max_folder_bytes: 10_000,
        };
        let dir = TempDir::new("logs-prune-fail");
        undeletable(dir.path(), "ordin.2026-11-15.log");
        put(dir.path(), "ordin.2026-11-16.log", 1);
        put(dir.path(), "ordin.2026-11-17.log", 1);
        let (now, clock) = clock("2026-11-18");
        let (mut files, not_deleted) =
            LogFiles::open_with_clock(dir.path(), limits, clock).expect("open");
        assert_eq!(not_deleted, ["ordin.2026-11-15.log"]);
        assert!(dir.path().join("ordin.2026-11-15.log").exists());
        assert!(!dir.path().join("ordin.2026-11-16.log").exists());
        assert!(!dir.path().join("ordin.2026-11-17.log").exists());
        *now.lock().expect("clock") = "2026-11-19".into();
        files.write_all(b"x\n").expect("write");
        assert!(dir.path().join("ordin.2026-11-15.log").exists());
        assert!(!dir.path().join("ordin.2026-11-18.log").exists());
        assert_eq!(files.current_file_name(), "ordin.2026-11-19.log");
    }

    #[test]
    fn a_folder_under_today_s_name_is_not_written_to() {
        let dir = TempDir::new("logs-today-folder");
        undeletable(dir.path(), "ordin.2026-11-18.log");
        let (_, clock) = clock("2026-11-18");
        let (files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        assert_eq!(files.current_file_name(), "ordin.2026-11-18.1.log");
    }

    fn records(bytes: &[u8]) -> Vec<serde_json::Value> {
        String::from_utf8(bytes.to_vec())
            .expect("utf-8")
            .lines()
            .map(|l| serde_json::from_str(l).expect("a JSON line"))
            .collect()
    }

    fn log_lines(bytes: &[u8]) -> Vec<(String, String)> {
        records(bytes)
            .into_iter()
            .filter(|r| r["record"] == "log_line")
            .map(|r| {
                (
                    r["file"].as_str().expect("file").to_owned(),
                    r["line"].as_str().expect("line").to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn the_days_back_cross_a_month_and_a_year() {
        assert_eq!(
            days_back("2026-03-01", 3),
            ["2026-03-01", "2026-02-28", "2026-02-27"]
        );
        assert_eq!(
            days_back("2027-01-01", 3),
            ["2027-01-01", "2026-12-31", "2026-12-30"]
        );
    }

    // Scenario: Happy path — the last 3 days of logs are in the support file (DSK-LOG-5); Edge case —
    // the file being written is read up to its last whole line.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-5
    #[test]
    fn the_support_file_holds_the_last_three_days_oldest_first_and_whole_lines_only() {
        let dir = TempDir::new("logs-support");
        fs::write(dir.path().join("ordin.2026-11-15.log"), "too old\n").expect("write");
        fs::write(dir.path().join("ordin.2026-11-16.log"), "sixteen\n").expect("write");
        fs::write(dir.path().join("ordin.2026-11-17.log"), "seventeen\n\n").expect("write");
        fs::write(dir.path().join("ordin.2026-11-18.log"), "eighteen\n").expect("write");
        fs::write(dir.path().join("notes.txt"), "not a log\n").expect("write");
        let limits = Limits {
            max_files: 14,
            max_file_bytes: 9,
            max_folder_bytes: 10_000,
        };
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), limits, clock).expect("open");
        assert_eq!(files.current_file_name(), "ordin.2026-11-18.1.log");
        files.write_all(b"current\n").expect("write");
        // A line written straight to the file, past the writer's length, is not read.
        let mut raw = OpenOptions::new()
            .append(true)
            .open(dir.path().join("ordin.2026-11-18.1.log"))
            .expect("open");
        raw.write_all(b"half a li").expect("write");
        let logs = support_logs(Some(&files), None);
        assert_eq!(logs.files, 4);
        assert!(logs.unreadable.is_empty());
        assert_eq!(
            log_lines(&logs.bytes),
            [
                ("ordin.2026-11-16.log".to_owned(), "sixteen".to_owned()),
                ("ordin.2026-11-17.log".to_owned(), "seventeen".to_owned()),
                ("ordin.2026-11-18.log".to_owned(), "eighteen".to_owned()),
                ("ordin.2026-11-18.1.log".to_owned(), "current".to_owned()),
            ]
        );
        assert!(records(&logs.bytes)
            .iter()
            .all(|r| r["record"] == "log_line"));
        // Logging carries on.
        files.write_all(b"after\n").expect("write");
    }

    // Scenario: Edge case — the newest lines are kept (DSK-LOG-5).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-5
    #[test]
    fn past_the_byte_limit_the_newest_lines_are_kept_after_a_cut_record() {
        let dir = TempDir::new("logs-support-cut");
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        for n in 0..10 {
            files
                .write_all(format!("line {n}\n").as_bytes())
                .expect("write");
        }
        let one = record(serde_json::json!({
            "record": "log_line",
            "file": files.current_file_name(),
            "line": "line 0",
        }))
        .len();
        let logs = files.support_logs(64 + 3 * one);
        let all = records(&logs.bytes);
        assert_eq!(all[0]["record"], "log_lines_cut");
        assert_eq!(all[0]["left_out"], 7);
        assert_eq!(
            log_lines(&logs.bytes)
                .into_iter()
                .map(|(_, line)| line)
                .collect::<Vec<_>>(),
            ["line 7", "line 8", "line 9"]
        );
        assert!(logs.bytes.len() <= 64 + 3 * one);
    }

    // Scenario: Error path — a log file cannot be read (DSK-LOG-5, DSK_SUPPORT_LOG_UNREADABLE): it is
    // left out and named; the rest is still read.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-5
    #[test]
    fn a_log_file_that_cannot_be_read_is_named_and_left_out() {
        let dir = TempDir::new("logs-support-unreadable");
        undeletable(dir.path(), "ordin.2026-11-17.log");
        fs::write(dir.path().join("ordin.2026-11-16.log"), "sixteen\n").expect("write");
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        files.write_all(b"today\n").expect("write");
        let logs = files.support_logs(SUPPORT_LOG_BYTES);
        assert_eq!(logs.files, 2);
        assert_eq!(logs.unreadable, ["ordin.2026-11-17.log"]);
        let all = records(&logs.bytes);
        assert_eq!(
            all.last().expect("a record"),
            &serde_json::json!({ "record": "log_file_unreadable", "file": "ordin.2026-11-17.log" })
        );
        assert_eq!(log_lines(&logs.bytes).len(), 2);
    }

    // Happy path: a window keeps the lines at or after its start, and lines with no time (DSK-LOG-6).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-LOG-6
    #[test]
    fn a_window_keeps_only_its_lines_and_lines_with_no_time() {
        let dir = TempDir::new("logs-support-window");
        let (_, clock) = clock("2026-11-18");
        let (mut files, _) = LogFiles::open_with_clock(dir.path(), SMALL, clock).expect("open");
        files
            .write_all(
                b"{\"time\":\"2026-11-18T10:00:00.000Z\",\"event\":\"old\"}\n\
                  {\"time\":\"2026-11-18T10:05:00.000Z\",\"event\":\"edge\"}\n\
                  {\"time\":\"2026-11-18T10:07:30.000Z\",\"event\":\"new\"}\n\
                  a line with no time\n",
            )
            .expect("write");
        let logs = files.support_logs_since(SUPPORT_LOG_BYTES, Some("2026-11-18T10:05:00.000Z"));
        let all = records(&logs.bytes);
        assert_eq!(
            all[0],
            serde_json::json!({ "record": "log_window", "since": "2026-11-18T10:05:00.000Z" })
        );
        let kept = log_lines(&logs.bytes);
        assert_eq!(kept.len(), 3, "{kept:?}");
        assert!(kept.iter().all(|(_, l)| !l.contains("\"old\"")), "{kept:?}");
        let every = files.support_logs(SUPPORT_LOG_BYTES);
        assert_eq!(
            log_lines(&every.bytes).len(),
            4,
            "no window keeps every line"
        );
    }

    // Error path: with no log open the log part says so and holds no line (DSK-LOG-5).
    #[test]
    fn with_no_log_open_the_log_part_says_so() {
        let logs = support_logs(None, None);
        assert_eq!(
            records(&logs.bytes),
            [serde_json::json!({ "record": "logs_not_open" })]
        );
        assert_eq!(logs.files, 0);
    }

    // Logging.md L-1 and R-2 to R-5 on the desktop: the lines step 2 writes, and so the lines the
    // support file copies (DSK-LOG-5), drop a forbidden field, write a `Display` or `Debug` value as
    // `<debug>`, and leave out a library's INFO line and a library's field values.
    // spec: ordin-desktop/cross-cutting/Logging#L-1
    #[test]
    fn the_desktop_s_lines_drop_forbidden_fields_and_never_hold_a_value_s_text() {
        let dir = TempDir::new("logs-format");
        let (files, _) = LogFiles::open(dir.path(), Limits::DESKTOP).expect("open");
        let (lines, guard) = subscriber(files.clone());
        tracing::subscriber::with_default(lines, || {
            let error = io::Error::other("the text of a document");
            let detail = vec!["a conversation line"];
            tracing::warn!(
                component = "desktop",
                connection = "tickets",
                bot_token = "xoxb-1",
                error = %error,
                detail = ?detail,
                "connection_bound"
            );
            tracing::info!(target: "hyper::client", "a library's info line");
            tracing::warn!(target: "hyper::client", address = "10.0.0.1", "a library's warning");
        });
        // Dropping the guard writes every queued line before the file is read.
        drop(guard);
        let logs = files.support_logs(SUPPORT_LOG_BYTES);
        let written: Vec<serde_json::Value> = log_lines(&logs.bytes)
            .into_iter()
            .map(|(_, line)| serde_json::from_str(&line).expect("a JSON line"))
            .collect();
        assert_eq!(written.len(), 2, "{written:?}");
        let first = &written[0];
        assert_eq!(first["level"], "WARNING");
        assert_eq!(first["event"], "connection_bound");
        assert_eq!(first["connection"], "tickets");
        assert_eq!(first["error"], "<debug>");
        assert_eq!(first["detail"], "<debug>");
        assert_eq!(first["dropped_fields"], serde_json::json!(["bot_token"]));
        let second = &written[1];
        assert_eq!(second["event"], "library_record");
        assert_eq!(second["template"], "a library's warning");
        assert!(second.get("address").is_none());
        let text = String::from_utf8_lossy(&logs.bytes);
        for secret in [
            "xoxb-1",
            "the text of a document",
            "a conversation line",
            "10.0.0.1",
            "a library's info line",
        ] {
            assert!(!text.contains(secret), "{secret}");
        }
    }
}
