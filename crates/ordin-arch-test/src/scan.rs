//! The person-only source scan (ADR-110, ADR-111): no agent-reachable code names a command only a
//! person may send, or calls a Team Knowledge adopt method.
//!
//! A source scan rather than a crate-graph rule: the programs depend on every crate, so the graph
//! cannot tell a Service API dispatch from a tool handler. The tokeniser is this crate's own; no
//! parser library is added.

use crate::{Metadata, Report, Violation};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The methods of the Team Knowledge Service only a person may start.
pub const ADOPT_METHODS: &[&str] = &["adopt", "adopt_again", "keep_as_is"];

/// The crate that holds the catalogue and `PERSON_ONLY_COMMANDS`.
const CATALOGUE_CRATE: &str = "ordin-service-api";
/// The build-only crate that holds the command names as test data and ships in no program, so no
/// agent can reach it (*chosen here*: ArchitectureTest.md does not name it as a site).
const BUILD_ONLY_CRATE: &str = "ordin-arch-test";
/// The crate whose adopt methods are guarded.
const TEAM_KNOWLEDGE_CRATE: &str = "ordin-team-knowledge";
/// The programs whose `service_api` module registers the Service API dispatch (SA-ARCH-ADOPT-01).
const DISPATCH_PROGRAMS: &[&str] = &["ordin-desktop", "ordin-server"];

/// What the tokeniser found in one source file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    /// String literal values with their 1-based line.
    pub strings: Vec<(String, usize)>,
    /// Identifiers that follow a `.` and precede `(`, with their 1-based line.
    pub method_calls: Vec<(String, usize)>,
}

/// The file ends inside a string literal or a block comment.
#[derive(Debug, PartialEq, Eq)]
pub struct Unreadable;

/// Reads Rust source as text. Skips `//` and `/* … */` comments (block comments nest, doc comments
/// included), reads `"…"` literals with escapes and raw strings `r"…"` to `r#"…"#` with up to 8
/// `#`, and records each identifier that follows a `.` and precedes `(`.
pub fn tokenise(source: &str) -> Result<Tokens, Unreadable> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Tokens::default();
    let mut i = 0;
    let mut line = 1usize;
    // The previous significant token was a `.`.
    let mut after_dot = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                let mut depth = 1;
                i += 2;
                while depth > 0 {
                    match (chars.get(i), chars.get(i + 1)) {
                        (None, _) => return Err(Unreadable),
                        (Some('/'), Some('*')) => {
                            depth += 1;
                            i += 2;
                        }
                        (Some('*'), Some('/')) => {
                            depth -= 1;
                            i += 2;
                        }
                        (Some('\n'), _) => {
                            line += 1;
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
            '"' => {
                let start_line = line;
                let (value, next, lines) = read_string(&chars, i + 1)?;
                out.strings.push((value, start_line));
                line += lines;
                i = next;
                after_dot = false;
            }
            'r' | 'b' if is_string_prefix(&chars, i) => {
                let start_line = line;
                let (value, next, lines) = read_prefixed_string(&chars, i)?;
                out.strings.push((value, start_line));
                line += lines;
                i = next;
                after_dot = false;
            }
            '\'' => {
                i = skip_char_or_lifetime(&chars, i);
                after_dot = false;
            }
            '.' => {
                after_dot = true;
                i += 1;
            }
            c if c == '_' || c.is_alphabetic() => {
                let start = i;
                while i < chars.len() && (chars[i] == '_' || chars[i].is_alphanumeric()) {
                    i += 1;
                }
                if after_dot {
                    let mut j = i;
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if chars.get(j) == Some(&'(') {
                        out.method_calls
                            .push((chars[start..i].iter().collect(), line));
                    }
                }
                after_dot = false;
            }
            _ => {
                after_dot = false;
                i += 1;
            }
        }
    }
    Ok(out)
}

/// `r"`, `r#"`, `b"`, `br"`, `br#"` at `i`, and not the tail of a longer identifier.
fn is_string_prefix(chars: &[char], i: usize) -> bool {
    if i > 0 && (chars[i - 1] == '_' || chars[i - 1].is_alphanumeric()) {
        return false;
    }
    let mut j = i;
    if chars[j] == 'b' {
        j += 1;
        if chars.get(j) == Some(&'"') {
            return true;
        }
    }
    if chars.get(j) != Some(&'r') {
        return false;
    }
    j += 1;
    let mut hashes = 0;
    while chars.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    hashes <= 8 && chars.get(j) == Some(&'"')
}

fn read_prefixed_string(chars: &[char], i: usize) -> Result<(String, usize, usize), Unreadable> {
    let mut j = i;
    if chars[j] == 'b' {
        j += 1;
        if chars[j] == '"' {
            return read_string(chars, j + 1);
        }
    }
    j += 1; // the `r`
    let mut hashes = 0;
    while chars[j] == '#' {
        hashes += 1;
        j += 1;
    }
    j += 1; // the opening quote
    let mut value = String::new();
    let mut lines = 0;
    while j < chars.len() {
        if chars[j] == '"' && (0..hashes).all(|k| chars.get(j + 1 + k) == Some(&'#')) {
            return Ok((value, j + 1 + hashes, lines));
        }
        if chars[j] == '\n' {
            lines += 1;
        }
        value.push(chars[j]);
        j += 1;
    }
    Err(Unreadable)
}

/// Reads a `"…"` body from just after the opening quote; returns the value, the index after the
/// closing quote and the number of newlines inside.
fn read_string(chars: &[char], mut i: usize) -> Result<(String, usize, usize), Unreadable> {
    let mut value = String::new();
    let mut lines = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => return Ok((value, i + 1, lines)),
            '\\' => {
                match chars.get(i + 1) {
                    None => return Err(Unreadable),
                    Some('n') => value.push('\n'),
                    Some('t') => value.push('\t'),
                    Some('r') => value.push('\r'),
                    Some('0') => value.push('\0'),
                    Some('\n') => {
                        // A line continuation skips the newline and the next line's indentation.
                        lines += 1;
                        i += 2;
                        while i < chars.len() && chars[i].is_whitespace() {
                            if chars[i] == '\n' {
                                lines += 1;
                            }
                            i += 1;
                        }
                        continue;
                    }
                    Some(other) => value.push(*other),
                }
                i += 2;
            }
            '\n' => {
                lines += 1;
                value.push('\n');
                i += 1;
            }
            c => {
                value.push(c);
                i += 1;
            }
        }
    }
    Err(Unreadable)
}

/// At a `'`: skips a character literal, or the lifetime tick, and returns the next index.
fn skip_char_or_lifetime(chars: &[char], i: usize) -> usize {
    match (chars.get(i + 1), chars.get(i + 2)) {
        (Some('\\'), _) => {
            let mut j = i + 2;
            while j < chars.len() && chars[j] != '\'' && chars[j] != '\n' {
                j += 1;
            }
            (j + 1).min(chars.len())
        }
        (Some(_), Some('\'')) => i + 3,
        _ => i + 1,
    }
}

/// One source file to scan: its crate, its path relative to the crate directory (forward slashes),
/// and its text.
#[derive(Debug)]
pub struct SourceFile {
    pub krate: String,
    pub relative: String,
    pub text: Result<String, ()>,
}

/// Whether a file is a site SA-ARCH-ADOPT-01 allows to name the commands: the catalogue crate, and
/// the `service_api` module of each program.
fn allowed_site(file: &SourceFile) -> bool {
    if file.krate == CATALOGUE_CRATE || file.krate == BUILD_ONLY_CRATE {
        return true;
    }
    DISPATCH_PROGRAMS.contains(&file.krate.as_str())
        && (file.relative == "src/service_api.rs" || file.relative.starts_with("src/service_api/"))
}

/// Scans the given files. `depends_on_team_knowledge` is the set of crates with a dependency on
/// `ordin-team-knowledge`; the adopt-method rule applies to their files only.
pub fn check_sources(
    files: &[SourceFile],
    commands: &[&str],
    depends_on_team_knowledge: &BTreeSet<String>,
) -> Report {
    let mut report = Report::default();
    for file in files {
        let display = format!("{}/{}", file.krate, file.relative);
        let tokens = match file
            .text
            .as_ref()
            .map_err(|_| Unreadable)
            .and_then(|t| tokenise(t))
        {
            Ok(tokens) => tokens,
            Err(Unreadable) => {
                report
                    .violations
                    .push(Violation::SourceUnreadable { file: display });
                continue;
            }
        };
        if allowed_site(file) {
            continue;
        }
        for (value, line) in &tokens.strings {
            if commands.contains(&value.as_str()) {
                report.violations.push(Violation::PersonOnlyNamed {
                    krate: file.krate.clone(),
                    file: display.clone(),
                    line: *line,
                    command: value.clone(),
                });
            }
        }
        if file.krate != TEAM_KNOWLEDGE_CRATE && depends_on_team_knowledge.contains(&file.krate) {
            for (name, line) in &tokens.method_calls {
                if ADOPT_METHODS.contains(&name.as_str()) {
                    report.violations.push(Violation::PersonOnlyCalled {
                        krate: file.krate.clone(),
                        file: display.clone(),
                        line: *line,
                        method: name.clone(),
                    });
                }
            }
        }
    }
    crate::normalise(&mut report.violations);
    report
}

/// Scans every `.rs` file of the workspace's member crates (the directory of each member's
/// `manifest_path`, excluding `target/` and `tests/fixtures/`) and returns one violation per
/// offending site.
pub fn check_person_only(metadata: &Metadata, commands: &[&str]) -> Report {
    let names: std::collections::BTreeMap<&str, &str> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();
    let members: Vec<&crate::Package> = metadata
        .packages
        .iter()
        .filter(|p| metadata.workspace_members.contains(&p.id))
        .collect();

    let mut dependents = BTreeSet::new();
    if let Some(resolve) = &metadata.resolve {
        for node in &resolve.nodes {
            let Some(from) = names.get(node.id.as_str()) else {
                continue;
            };
            if node
                .deps
                .iter()
                .any(|d| names.get(d.pkg.as_str()) == Some(&TEAM_KNOWLEDGE_CRATE))
            {
                dependents.insert((*from).to_string());
            }
        }
    }

    let mut files = Vec::new();
    for package in members {
        let Some(dir) = Path::new(&package.manifest_path).parent() else {
            continue;
        };
        let mut found = Vec::new();
        collect_rs_files(dir, dir, &mut found);
        found.sort();
        for path in found {
            let relative = path
                .strip_prefix(dir)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            files.push(SourceFile {
                krate: package.name.clone(),
                relative,
                text: std::fs::read_to_string(&path).map_err(|_| ()),
            });
        }
    }
    check_sources(&files, commands, &dependents)
}

fn collect_rs_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            if relative == "target" || relative == "tests/fixtures" {
                continue;
            }
            collect_rs_files(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
