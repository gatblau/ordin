//! The architecture test: holds the crate graph of the workspace to Phase 2B-3.
//!
//! The library reads the subset of `cargo metadata` it needs, compares every edge between workspace
//! crates with the compiled-in [`ALLOWED`] table, and returns every violation in one [`Report`].
//! The person-only source scan lives in [`scan`].
#![forbid(unsafe_code)]

pub mod scan;
mod table;

pub use table::ALLOWED;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Every error and violation code of this crate (ErrorHandling.md, prefix `ARCH_`).
pub const ALL_CODES: &[&str] = &[
    "ARCH_CYCLE",
    "ARCH_DEV_EDGE_NOT_ALLOWED",
    "ARCH_EDGE_NOT_ALLOWED",
    "ARCH_EMBEDDED_EDGE",
    "ARCH_METADATA_FAILED",
    "ARCH_METADATA_INVALID",
    "ARCH_PERSON_ONLY_CALLED",
    "ARCH_PERSON_ONLY_NAMED",
    "ARCH_SOURCE_UNREADABLE",
    "ARCH_TABLE_INVALID",
    "ARCH_UNKNOWN_CRATE",
    "ARCH_UNKNOWN_PATH_PACKAGE",
];

/// The program that depends on every module of its program (2B-1 rule 3).
const DESKTOP_PROGRAM: &str = "ordin-desktop";
const SERVER_PROGRAM: &str = "ordin-server";
/// The crate every workspace crate may depend on (2B-3).
const SHARED_TYPES: &str = "ordin-types";
/// The phase of the programs, which no program-wide allowance reaches.
const PROGRAM_PHASE: u8 = 12;

/// One row of the allowed graph (2B-3).
#[derive(Debug, Clone, Copy)]
pub struct AllowedCrate {
    pub name: &'static str,
    pub phase: u8,
    pub programs: Programs,
    /// Direct workspace dependencies allowed besides `ordin-types`.
    pub deps: AllowedDeps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Programs {
    pub desktop: bool,
    pub personal: bool,
    pub agent: bool,
    pub build_only: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum AllowedDeps {
    /// The listed crates.
    List(&'static [&'static str]),
    /// Every crate of the table with `programs.desktop` (the desktop program).
    EveryDesktopModule,
    /// Every crate of the table with `programs.personal || programs.agent` (the server program).
    EveryServerModule,
}

/// Embedded path packages and the one crate allowed to depend on each.
pub const EMBEDDED: &[(&str, &str)] = &[
    ("memory", "ordin-memory-service"),
    ("sandboxd", "ordin-sandbox"),
    ("sandboxd-ipc", "sandboxd"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    Normal,
    Build,
    Dev,
}

impl EdgeKind {
    fn word(self) -> &'static str {
        match self {
            EdgeKind::Normal => "normal",
            EdgeKind::Build => "build",
            EdgeKind::Dev => "dev",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Violation {
    UnknownCrate {
        name: String,
    },
    EdgeNotAllowed {
        from: String,
        to: String,
        kind: EdgeKind,
    },
    DevEdgeNotAllowed {
        from: String,
        to: String,
    },
    Cycle {
        members: Vec<String>,
    },
    EmbeddedEdge {
        from: String,
        to: String,
    },
    UnknownPathPackage {
        name: String,
    },
    PersonOnlyNamed {
        krate: String,
        file: String,
        line: usize,
        command: String,
    },
    PersonOnlyCalled {
        krate: String,
        file: String,
        line: usize,
        method: String,
    },
    SourceUnreadable {
        file: String,
    },
}

impl Violation {
    /// The registry code of the violation.
    pub fn code(&self) -> &'static str {
        match self {
            Violation::UnknownCrate { .. } => "ARCH_UNKNOWN_CRATE",
            Violation::EdgeNotAllowed { .. } => "ARCH_EDGE_NOT_ALLOWED",
            Violation::DevEdgeNotAllowed { .. } => "ARCH_DEV_EDGE_NOT_ALLOWED",
            Violation::Cycle { .. } => "ARCH_CYCLE",
            Violation::EmbeddedEdge { .. } => "ARCH_EMBEDDED_EDGE",
            Violation::UnknownPathPackage { .. } => "ARCH_UNKNOWN_PATH_PACKAGE",
            Violation::PersonOnlyNamed { .. } => "ARCH_PERSON_ONLY_NAMED",
            Violation::PersonOnlyCalled { .. } => "ARCH_PERSON_ONLY_CALLED",
            Violation::SourceUnreadable { .. } => "ARCH_SOURCE_UNREADABLE",
        }
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = self.code();
        match self {
            Violation::UnknownCrate { name } | Violation::UnknownPathPackage { name } => {
                write!(f, "{code} {name}")
            }
            Violation::EdgeNotAllowed { from, to, kind } => {
                write!(f, "{code} {from} -> {to} ({})", kind.word())
            }
            Violation::DevEdgeNotAllowed { from, to } | Violation::EmbeddedEdge { from, to } => {
                write!(f, "{code} {from} -> {to}")
            }
            Violation::Cycle { members } => write!(f, "{code} {}", members.join(" -> ")),
            Violation::PersonOnlyNamed {
                krate,
                file,
                line,
                command,
            } => write!(
                f,
                "{code} {krate} {file}:{line} names {command}, which only a person may send"
            ),
            Violation::PersonOnlyCalled {
                krate,
                file,
                line,
                method,
            } => write!(
                f,
                "{code} {krate} {file}:{line} calls {method}, which only a person may start"
            ),
            Violation::SourceUnreadable { file } => write!(f, "{code} {file} could not be read"),
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// Sorted by code, then by the rendered line; de-duplicated.
    pub violations: Vec<Violation>,
    /// Rows of `ALLOWED` with no workspace member yet (the Generation Playbook builds in phases).
    pub not_yet_built: Vec<&'static str>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }

    /// Joins another report's violations into this one and re-sorts.
    pub fn merge(&mut self, other: Report) {
        self.violations.extend(other.violations);
        normalise(&mut self.violations);
    }
}

fn normalise(violations: &mut Vec<Violation>) {
    violations.sort_by_cached_key(|v| (v.code(), v.to_string()));
    violations.dedup();
}

#[derive(Debug, thiserror::Error)]
pub enum ArchTestError {
    #[error("ARCH_METADATA_FAILED: cargo metadata exited with {status}: {stderr_first_line}")]
    MetadataFailed {
        status: String,
        stderr_first_line: String,
    },
    #[error("ARCH_METADATA_INVALID: {0}")]
    MetadataInvalid(String),
    #[error("ARCH_TABLE_INVALID: {0}")]
    TableInvalid(String),
}

impl ArchTestError {
    pub fn code(&self) -> &'static str {
        match self {
            ArchTestError::MetadataFailed { .. } => "ARCH_METADATA_FAILED",
            ArchTestError::MetadataInvalid(_) => "ARCH_METADATA_INVALID",
            ArchTestError::TableInvalid(_) => "ARCH_TABLE_INVALID",
        }
    }
}

/// Deserialised subset of `cargo metadata --format-version 1`.
#[derive(Debug, serde::Deserialize)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub resolve: Option<Resolve>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub source: Option<String>,
    pub manifest_path: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Node {
    pub id: String,
    pub deps: Vec<NodeDep>,
}

#[derive(Debug, serde::Deserialize)]
pub struct NodeDep {
    pub pkg: String,
    pub dep_kinds: Vec<DepKindInfo>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DepKindInfo {
    pub kind: Option<String>,
}

/// Runs `$CARGO metadata --format-version 1 --locked --all-features --manifest-path <workspace>/Cargo.toml`
/// and parses its standard output.
///
/// `--all-features` keeps the programs' optional dependencies behind build switches in the checked
/// graph; `cargo metadata` resolves and never compiles.
pub fn read_metadata(workspace_manifest: &Path) -> Result<Metadata, ArchTestError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--all-features",
            "--manifest-path",
        ])
        .arg(workspace_manifest)
        .output()
        .map_err(|e| ArchTestError::MetadataFailed {
            status: "no exit status".to_string(),
            stderr_first_line: e.to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ArchTestError::MetadataFailed {
            status: output.status.to_string(),
            stderr_first_line: stderr.lines().next().unwrap_or("").to_string(),
        });
    }
    parse_metadata(&output.stdout)
}

/// Parses the standard output of `cargo metadata`; a missing `resolve` is invalid.
pub fn parse_metadata(stdout: &[u8]) -> Result<Metadata, ArchTestError> {
    let metadata: Metadata = serde_json::from_slice(stdout)
        .map_err(|e| ArchTestError::MetadataInvalid(e.to_string()))?;
    if metadata.resolve.is_none() {
        return Err(ArchTestError::MetadataInvalid(
            "resolve is null".to_string(),
        ));
    }
    Ok(metadata)
}

/// Checks the table itself: unique names, every listed dependency is a row of a strictly lower
/// phase, `ordin-types` is phase 0 with no dependency.
pub fn check_table(table: &[AllowedCrate]) -> Result<(), ArchTestError> {
    let mut phases: BTreeMap<&str, u8> = BTreeMap::new();
    for row in table {
        if phases.insert(row.name, row.phase).is_some() {
            return Err(ArchTestError::TableInvalid(format!(
                "duplicate row {}",
                row.name
            )));
        }
    }
    match table.iter().find(|r| r.name == SHARED_TYPES) {
        Some(row) if row.phase == 0 && matches!(row.deps, AllowedDeps::List(l) if l.is_empty()) => {
        }
        _ => {
            return Err(ArchTestError::TableInvalid(format!(
                "{SHARED_TYPES} must be the row of phase 0 with no dependency"
            )))
        }
    }
    for row in table {
        match row.deps {
            AllowedDeps::List(deps) => {
                for dep in deps {
                    let Some(dep_phase) = phases.get(dep) else {
                        return Err(ArchTestError::TableInvalid(format!(
                            "{} -> {dep}: not a row of the table",
                            row.name
                        )));
                    };
                    if *dep_phase >= row.phase {
                        return Err(ArchTestError::TableInvalid(format!(
                            "{} -> {dep}: phase {dep_phase} is not below {}",
                            row.name, row.phase
                        )));
                    }
                }
            }
            AllowedDeps::EveryDesktopModule if row.name != DESKTOP_PROGRAM => {
                return Err(ArchTestError::TableInvalid(format!(
                    "{}: EveryDesktopModule belongs to {DESKTOP_PROGRAM} only",
                    row.name
                )))
            }
            AllowedDeps::EveryServerModule if row.name != SERVER_PROGRAM => {
                return Err(ArchTestError::TableInvalid(format!(
                    "{}: EveryServerModule belongs to {SERVER_PROGRAM} only",
                    row.name
                )))
            }
            _ => {}
        }
    }
    Ok(())
}

fn row<'a>(table: &'a [AllowedCrate], name: &str) -> Option<&'a AllowedCrate> {
    table.iter().find(|r| r.name == name)
}

fn allows_normal(source: &AllowedCrate, target: &AllowedCrate) -> bool {
    match source.deps {
        AllowedDeps::List(list) => list.contains(&target.name),
        AllowedDeps::EveryDesktopModule => target.programs.desktop && target.phase < PROGRAM_PHASE,
        AllowedDeps::EveryServerModule => {
            (target.programs.personal || target.programs.agent) && target.phase < PROGRAM_PHASE
        }
    }
}

/// Compares the metadata with the table and returns every violation.
pub fn check(metadata: &Metadata, table: &[AllowedCrate]) -> Report {
    let names: BTreeMap<&str, &str> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();
    let members: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .filter_map(|id| names.get(id.as_str()).copied())
        .collect();
    let path_packages: BTreeSet<&str> = metadata
        .packages
        .iter()
        .filter(|p| p.source.is_none() && !members.contains(p.name.as_str()))
        .map(|p| p.name.as_str())
        .collect();

    let mut report = Report::default();

    for name in &members {
        if row(table, name).is_none() {
            report.violations.push(Violation::UnknownCrate {
                name: (*name).to_string(),
            });
        }
    }
    for name in &path_packages {
        if !EMBEDDED.iter().any(|(pkg, _)| pkg == name) {
            report.violations.push(Violation::UnknownPathPackage {
                name: (*name).to_string(),
            });
        }
    }
    report.not_yet_built = table
        .iter()
        .filter(|r| !members.contains(r.name))
        .map(|r| r.name)
        .collect();
    report.not_yet_built.sort_unstable();

    // Normal and build edges between members, allowed or not, for the cycle search.
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

    let nodes = metadata
        .resolve
        .as_ref()
        .map(|r| r.nodes.as_slice())
        .unwrap_or(&[]);
    for node in nodes {
        let Some(from) = names.get(node.id.as_str()).copied() else {
            continue;
        };
        if !members.contains(from) {
            continue;
        }
        let source = row(table, from);
        for dep in &node.deps {
            let Some(to) = names.get(dep.pkg.as_str()).copied() else {
                continue;
            };
            let to_member = members.contains(to);
            let to_embedded =
                path_packages.contains(to) && EMBEDDED.iter().any(|(pkg, _)| *pkg == to);
            if !to_member && !to_embedded {
                continue; // an edge to a registry crate
            }
            let kinds: BTreeSet<EdgeKind> = if dep.dep_kinds.is_empty() {
                BTreeSet::from([EdgeKind::Normal])
            } else {
                dep.dep_kinds
                    .iter()
                    .map(|k| match k.kind.as_deref() {
                        Some("build") => EdgeKind::Build,
                        Some("dev") => EdgeKind::Dev,
                        _ => EdgeKind::Normal,
                    })
                    .collect()
            };
            for kind in kinds {
                if kind != EdgeKind::Dev && to_member {
                    graph.entry(from).or_default().insert(to);
                }
                if to == SHARED_TYPES {
                    continue;
                }
                if to_embedded {
                    let owner_ok = EMBEDDED
                        .iter()
                        .any(|(pkg, owner)| *pkg == to && *owner == from);
                    if !owner_ok {
                        report.violations.push(Violation::EmbeddedEdge {
                            from: from.to_string(),
                            to: to.to_string(),
                        });
                    }
                    continue;
                }
                let (Some(source), Some(target)) = (source, row(table, to)) else {
                    continue;
                };
                match kind {
                    EdgeKind::Normal | EdgeKind::Build => {
                        if !allows_normal(source, target) {
                            report.violations.push(Violation::EdgeNotAllowed {
                                from: from.to_string(),
                                to: to.to_string(),
                                kind,
                            });
                        }
                    }
                    EdgeKind::Dev => {
                        if target.phase >= source.phase {
                            report.violations.push(Violation::DevEdgeNotAllowed {
                                from: from.to_string(),
                                to: to.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    for members in cycles(&graph) {
        report.violations.push(Violation::Cycle { members });
    }
    normalise(&mut report.violations);
    report
}

/// Tarjan's strongly connected components over the member graph. Each component of more than one
/// member, and each member with an edge to itself, is returned as a closed path that starts at the
/// alphabetically first member and follows edges.
fn cycles(graph: &BTreeMap<&str, BTreeSet<&str>>) -> Vec<Vec<String>> {
    struct State<'a> {
        graph: &'a BTreeMap<&'a str, BTreeSet<&'a str>>,
        index: BTreeMap<&'a str, usize>,
        low: BTreeMap<&'a str, usize>,
        on_stack: BTreeSet<&'a str>,
        stack: Vec<&'a str>,
        next: usize,
        components: Vec<Vec<&'a str>>,
    }
    fn visit<'a>(s: &mut State<'a>, v: &'a str) {
        s.index.insert(v, s.next);
        s.low.insert(v, s.next);
        s.next += 1;
        s.stack.push(v);
        s.on_stack.insert(v);
        let successors: Vec<&'a str> = s
            .graph
            .get(v)
            .map(|e| e.iter().copied().collect())
            .unwrap_or_default();
        for w in successors {
            if !s.index.contains_key(w) {
                visit(s, w);
                let low = s.low[v].min(s.low[w]);
                s.low.insert(v, low);
            } else if s.on_stack.contains(w) {
                let low = s.low[v].min(s.index[w]);
                s.low.insert(v, low);
            }
        }
        if s.low[v] == s.index[v] {
            let mut component = Vec::new();
            while let Some(w) = s.stack.pop() {
                s.on_stack.remove(w);
                component.push(w);
                if w == v {
                    break;
                }
            }
            s.components.push(component);
        }
    }

    let mut state = State {
        graph,
        index: BTreeMap::new(),
        low: BTreeMap::new(),
        on_stack: BTreeSet::new(),
        stack: Vec::new(),
        next: 0,
        components: Vec::new(),
    };
    let mut vertices: BTreeSet<&str> = graph.keys().copied().collect();
    for targets in graph.values() {
        vertices.extend(targets.iter().copied());
    }
    for v in vertices {
        if !state.index.contains_key(v) {
            visit(&mut state, v);
        }
    }

    let mut out = Vec::new();
    for component in state.components {
        let self_loop = component.len() == 1
            && graph
                .get(component[0])
                .is_some_and(|e| e.contains(component[0]));
        if component.len() < 2 && !self_loop {
            continue;
        }
        let set: BTreeSet<&str> = component.iter().copied().collect();
        let start = *set.iter().next().expect("component is not empty");
        out.push(path_back_to(graph, &set, start));
    }
    out
}

/// A closed path from `start` through `component` and back, by breadth-first search.
fn path_back_to(
    graph: &BTreeMap<&str, BTreeSet<&str>>,
    component: &BTreeSet<&str>,
    start: &str,
) -> Vec<String> {
    let mut previous: BTreeMap<&str, &str> = BTreeMap::new();
    let mut queue = std::collections::VecDeque::from([start]);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    while let Some(v) = queue.pop_front() {
        for &w in graph.get(v).into_iter().flatten() {
            if !component.contains(w) {
                continue;
            }
            if w == start {
                let mut path = vec![v.to_string()];
                let mut cursor = v;
                while cursor != start {
                    cursor = previous[cursor];
                    path.push(cursor.to_string());
                }
                path.reverse();
                path.push(start.to_string());
                return path;
            }
            if seen.insert(w) {
                previous.insert(w, v);
                queue.push_back(w);
            }
        }
    }
    vec![start.to_string(), start.to_string()]
}

#[cfg(test)]
mod tests;
