//! The team folder's files: names, the bytes this service writes, and the bounded, strict reading of
//! every file a colleague wrote (TeamKnowledgeService.md *Shared Context* §3, the ADR-118 format 2
//! and the team workspace amendment's Shared Context 2). Every file is data: it is parsed, checked
//! and compared, never run.

use std::sync::OnceLock;

use ordin_memory_service::{TeamEntity, TeamFact, TeamRelationship, TeamSource, TeamTerm};
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{KnowledgePageId, MemoryFactId, TeamActivityId};
use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// `<folder>/ordin-team-knowledge.json`.
pub const FORMAT_FILE: &str = "ordin-team-knowledge.json";
/// `<folder>/ordin-team-workspace.json` (team workspace amendment, Shared Context 2).
pub const DESCRIPTION_FILE: &str = "ordin-team-workspace.json";
/// The folder format this build writes and reads (TK-TR-1, TK-TR-2).
pub const FOLDER_FORMAT: u64 = 2;
/// The description format this build writes and reads.
pub const DESCRIPTION_FORMAT: u8 = 1;
/// Every item file (§3).
pub const ITEM_MAX_BYTES: usize = 65_536;
/// A page file: 1 MiB + 4 KiB of header (§3).
pub const PAGE_MAX_BYTES: usize = 1_048_576 + 4_096;
/// The description (Configuration additions).
pub const DESCRIPTION_MAX_BYTES: usize = 262_144;
/// An activity file (Configuration additions).
pub const ACTIVITY_MAX_BYTES: usize = 8_192;
/// 50 MiB (SA-TEAM-TW-10).
pub const TEAM_RESULT_MAX_BYTES: u64 = 52_428_800;
pub const LOGIN_PATTERN: &str = r"^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$";
pub const TEAM_CHANGES: &[&str] = &[
    "renamed",
    "place_added",
    "place_removed",
    "agent_added",
    "agent_removed",
    "access_changed",
    "results_place_changed",
];
/// At most 100 places and 50 agents in a description (Configuration additions).
pub const MAX_PLACES: usize = 100;
pub const MAX_AGENTS: usize = 50;

/// The item sub-folders the walk reads (§3; `terms` added by TK-TR-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKind {
    Source,
    Entity,
    Term,
    Relationship,
    Fact,
    Page,
}

impl ItemKind {
    /// The `team_knowledge_items.kind` value; terms are never recorded there (TK-TR-1).
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Fact => "fact",
            ItemKind::Entity => "entity",
            ItemKind::Source => "source",
            ItemKind::Relationship => "relationship",
            ItemKind::Page => "page",
            ItemKind::Term => "term",
        }
    }

    pub fn parse(raw: &str) -> Option<ItemKind> {
        Some(match raw {
            "fact" => ItemKind::Fact,
            "entity" => ItemKind::Entity,
            "source" => ItemKind::Source,
            "relationship" => ItemKind::Relationship,
            "page" => ItemKind::Page,
            "term" => ItemKind::Term,
            _ => return None,
        })
    }

    fn folder(self) -> &'static str {
        match self {
            ItemKind::Fact => "facts",
            ItemKind::Entity => "entities",
            ItemKind::Source => "sources",
            ItemKind::Relationship => "relationships",
            ItemKind::Page => "pages",
            ItemKind::Term => "terms",
        }
    }

    fn extension(self) -> &'static str {
        if self == ItemKind::Page {
            ".md"
        } else {
            ".json"
        }
    }

    /// The path of an item file relative to the folder; `stem` is the id without its prefix.
    pub fn path(self, stem: &str) -> String {
        format!("{}/{stem}{}", self.folder(), self.extension())
    }
}

/// What a path below the bound folder is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    Format,
    Description,
    Item { kind: ItemKind, stem: String },
    Activity { stem: String },
    Result { path: String },
}

/// §3: a file of any other name is ignored (`None`); the team workspace's names are recognised
/// only by this build (TW Approach).
pub fn classify(path: &str) -> Option<PathKind> {
    if path == FORMAT_FILE {
        return Some(PathKind::Format);
    }
    if path == DESCRIPTION_FILE {
        return Some(PathKind::Description);
    }
    if let Some(rest) = path.strip_prefix("results/") {
        return Some(PathKind::Result {
            path: rest.to_owned(),
        });
    }
    let (folder, name) = path.split_once('/')?;
    if name.contains('/') {
        return None;
    }
    if folder == "activity" {
        return name.strip_suffix(".json").map(|stem| PathKind::Activity {
            stem: stem.to_owned(),
        });
    }
    let kind = [
        ItemKind::Fact,
        ItemKind::Entity,
        ItemKind::Source,
        ItemKind::Relationship,
        ItemKind::Page,
        ItemKind::Term,
    ]
    .into_iter()
    .find(|k| k.folder() == folder)?;
    let stem = name.strip_suffix(kind.extension())?;
    if stem.is_empty() {
        return None;
    }
    Some(PathKind::Item {
        kind,
        stem: stem.to_owned(),
    })
}

/// `serde_json::to_string_pretty` (two-space indent), fields in declaration order, one `\n` (§3).
pub fn render<T: Serialize>(value: &T) -> Vec<u8> {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text.into_bytes()
}

/// The format file's bytes.
pub fn format_file_bytes() -> Vec<u8> {
    render(&serde_json::json!({ "format": FOLDER_FORMAT }))
}

/// The format a folder's format file names: `Ok(None)` when absent, `Err(())` when it is not
/// `{"format": n}` with `n` an integer (§D step 3).
pub fn read_format(bytes: Option<&[u8]>) -> Result<Option<u64>, ()> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if bytes.len() > ITEM_MAX_BYTES {
        return Err(());
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Format {
        format: u64,
    }
    serde_json::from_slice::<Format>(bytes)
        .map(|f| Some(f.format))
        .map_err(|_| ())
}

// ---- facts (format 2: every field written, `null` for an absent optional; TK-TR-1) ----

#[derive(Serialize)]
struct FactFile<'a> {
    format: u8,
    id: MemoryFactId,
    memory_fact_id: &'a str,
    statement: &'a str,
    about: &'a Option<String>,
    memory_class: &'a str,
    fact_class: &'a Option<String>,
    origin_kind: &'a str,
    source_id: &'a Option<String>,
    entities: &'a [String],
    valid_from: Timestamp,
    valid_to: Option<Timestamp>,
    confidence_permille: u16,
    supersedes: &'a Option<String>,
    superseded_by: &'a Option<String>,
    derived_from: &'a [String],
    written_at: Timestamp,
    shared_by: &'a str,
    shared_at: Timestamp,
    triple: &'a Option<ordin_memory_service::TeamTriple>,
    single_valued: bool,
    changed: bool,
}

// spec: ordin-desktop/components/TeamKnowledgeService#TK-TR-1
/// A fact's file bytes, with `shared_by` set to `name` when the fact was never shared (§E step 4).
pub fn render_fact(fact: &TeamFact, name: &str) -> Vec<u8> {
    let shared_by = if fact.shared_by.is_empty() {
        name
    } else {
        fact.shared_by.as_str()
    };
    render(&FactFile {
        format: 2,
        id: fact.id,
        memory_fact_id: &fact.memory_fact_id,
        statement: &fact.statement,
        about: &fact.about,
        memory_class: &fact.memory_class,
        fact_class: &fact.fact_class,
        origin_kind: &fact.origin_kind,
        source_id: &fact.source_id,
        entities: &fact.entities,
        valid_from: fact.valid_from,
        valid_to: fact.valid_to,
        confidence_permille: fact.confidence_permille,
        supersedes: &fact.supersedes,
        superseded_by: &fact.superseded_by,
        derived_from: &fact.derived_from,
        written_at: fact.written_at,
        shared_by,
        shared_at: fact.shared_at,
        triple: &fact.triple,
        single_valued: fact.single_valued,
        changed: fact.changed,
    })
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn bounded(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&chars(s))
}

fn prefixed_uuid(value: &str, prefix: &str) -> Option<uuid::Uuid> {
    value
        .strip_prefix(prefix)
        .and_then(|rest| uuid::Uuid::parse_str(rest).ok())
        .filter(|u| u.hyphenated().to_string() == value[prefix.len()..])
}

fn term_key_ok(key: &str) -> bool {
    static KEY: OnceLock<Regex> = OnceLock::new();
    KEY.get_or_init(|| Regex::new(r"^[a-z][A-Za-z0-9]{0,63}$").expect("pattern"))
        .is_match(key)
}

// spec: ordin-desktop/components/TeamKnowledgeService#TK-TR-2
/// A fact file read strictly: size bound, `deny_unknown_fields`, the file name equals the id, every
/// field rule of §3, and the triple's things among its own entities (TK-TR-2).
pub fn parse_fact(bytes: &[u8], stem: &str) -> Result<TeamFact, ()> {
    if bytes.len() > ITEM_MAX_BYTES {
        return Err(());
    }
    let fact: TeamFact = serde_json::from_slice(bytes).map_err(|_| ())?;
    let ok = (fact.format == 1 || fact.format == 2)
        && fact.id.to_string() == stem
        && prefixed_uuid(&fact.memory_fact_id, "fact:").is_some()
        && bounded(&fact.statement, 1, 4_000)
        && fact.about.as_deref().is_none_or(|a| bounded(a, 1, 200))
        && ["episodic", "semantic", "consolidated"].contains(&fact.memory_class.as_str())
        && fact
            .fact_class
            .as_deref()
            .is_none_or(|c| bounded(c, 1, 128))
        && ["conversation", "agent", "document", "person"].contains(&fact.origin_kind.as_str())
        && fact
            .source_id
            .as_deref()
            .is_none_or(|s| prefixed_uuid(s, "source:").is_some())
        && (1..=64).contains(&fact.entities.len())
        && fact
            .entities
            .iter()
            .all(|e| prefixed_uuid(e, "entity:").is_some())
        && fact.confidence_permille <= 1_000
        && fact
            .supersedes
            .as_deref()
            .is_none_or(|s| prefixed_uuid(s, "fact:").is_some())
        && fact
            .superseded_by
            .as_deref()
            .is_none_or(|s| prefixed_uuid(s, "fact:").is_some())
        && fact.derived_from.len() <= 64
        && bounded(&fact.shared_by, 1, 200);
    if !ok {
        return Err(());
    }
    if let Some(triple) = &fact.triple {
        if !term_key_ok(&triple.predicate) || !fact.entities.contains(&triple.subject) {
            return Err(());
        }
        if triple.object.get("kind").and_then(|k| k.as_str()) == Some("entity") {
            let named = triple.object.get("name").and_then(|n| n.as_str());
            if !named.is_some_and(|n| fact.entities.iter().any(|e| e == n)) {
                return Err(());
            }
        }
    }
    Ok(fact)
}

// ---- entities, sources, relationships (read with `deny_unknown_fields`) ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityFile {
    format: u8,
    id: String,
    canonical_name: String,
    aliases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    format: u8,
    id: String,
    origin_ref: String,
    modification_marker: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationshipFile {
    format: u8,
    id: String,
    kind: String,
    from: String,
    to: String,
    valid_from: Timestamp,
    valid_to: Option<Timestamp>,
    confidence_permille: u16,
    source_id: Option<String>,
}

fn stem_of(id: &str, prefix: &str, stem: &str) -> bool {
    prefixed_uuid(id, prefix).is_some() && &id[prefix.len()..] == stem
}

pub fn parse_entity(bytes: &[u8], stem: &str) -> Result<TeamEntity, ()> {
    if bytes.len() > ITEM_MAX_BYTES {
        return Err(());
    }
    let f: EntityFile = serde_json::from_slice(bytes).map_err(|_| ())?;
    let ok = f.format == 1
        && stem_of(&f.id, "entity:", stem)
        && bounded(&f.canonical_name, 1, 512)
        && f.aliases.len() <= 32;
    if !ok {
        return Err(());
    }
    Ok(TeamEntity {
        format: f.format,
        id: f.id,
        canonical_name: f.canonical_name,
        aliases: f.aliases,
    })
}

pub fn parse_source(bytes: &[u8], stem: &str) -> Result<TeamSource, ()> {
    if bytes.len() > ITEM_MAX_BYTES {
        return Err(());
    }
    let f: SourceFile = serde_json::from_slice(bytes).map_err(|_| ())?;
    let ok = f.format == 1
        && stem_of(&f.id, "source:", stem)
        && bounded(&f.origin_ref, 1, 2_048)
        && f.modification_marker
            .as_deref()
            .is_none_or(|m| bounded(m, 1, 256));
    if !ok {
        return Err(());
    }
    Ok(TeamSource {
        format: f.format,
        id: f.id,
        origin_ref: f.origin_ref,
        modification_marker: f.modification_marker,
    })
}

pub fn parse_relationship(bytes: &[u8], stem: &str) -> Result<TeamRelationship, ()> {
    if bytes.len() > ITEM_MAX_BYTES {
        return Err(());
    }
    let f: RelationshipFile = serde_json::from_slice(bytes).map_err(|_| ())?;
    let ok = f.format == 1
        && stem_of(&f.id, "relationship:", stem)
        && bounded(&f.kind, 1, 128)
        && !f.from.is_empty()
        && !f.to.is_empty()
        && f.confidence_permille <= 1_000;
    if !ok {
        return Err(());
    }
    Ok(TeamRelationship {
        format: f.format,
        id: f.id,
        kind: f.kind,
        from: f.from,
        to: f.to,
        valid_from: f.valid_from,
        valid_to: f.valid_to,
        confidence_permille: f.confidence_permille,
        source_id: f.source_id,
    })
}

/// A term file (TK-TR-2): bounds, the key equal to the file name.
pub fn parse_term(bytes: &[u8], stem: &str) -> Result<TeamTerm, ()> {
    if bytes.len() > ITEM_MAX_BYTES {
        return Err(());
    }
    let t: TeamTerm = serde_json::from_slice(bytes).map_err(|_| ())?;
    let ok = t.format == 2
        && t.key == stem
        && term_key_ok(&t.key)
        && bounded(&t.label, 1, 64)
        && bounded(&t.meaning, 1, 300)
        && ["thing", "text", "number", "money", "date", "yes_no"].contains(&t.object.as_str());
    if ok {
        Ok(t)
    } else {
        Err(())
    }
}

/// The page file read with the Knowledge Service's own parser: its page id must equal the file name.
/// Returns the sharer named in the header.
pub fn parse_page(bytes: &[u8], stem: &str) -> Result<(KnowledgePageId, Option<String>), ()> {
    if bytes.len() > PAGE_MAX_BYTES {
        return Err(());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let concept =
        ordin_knowledge::okf::parse_concept(&format!("pages/{stem}.md"), text).map_err(|_| ())?;
    let keys = concept.ordin.ok_or(())?;
    if keys.page_id.to_string() != stem {
        return Err(());
    }
    Ok((keys.page_id, keys.shared_by.filter(|s| !s.is_empty())))
}

/// §E step 4: the exact header line `shared_by: ""` takes the person's name, YAML double-quoted.
pub fn name_page(bytes: &[u8], name: &str) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return bytes.to_vec();
    };
    let mut out = String::with_capacity(text.len() + name.len());
    let mut in_header = false;
    let mut done = false;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        let bare = line.trim_end_matches('\n');
        if n == 0 && bare == "---" {
            in_header = true;
        } else if in_header && bare == "---" {
            in_header = false;
        } else if in_header && !done && bare == "shared_by: \"\"" {
            out.push_str("shared_by: ");
            out.push_str(&yaml_quoted(name));
            if line.ends_with('\n') {
                out.push('\n');
            }
            done = true;
            continue;
        }
        out.push_str(line);
    }
    out.into_bytes()
}

fn yaml_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The sharer a fact file names, for `available.from` (§D step 4).
pub fn sharer_of(kind: ItemKind, bytes: &[u8], stem: &str) -> Option<String> {
    match kind {
        ItemKind::Fact => parse_fact(bytes, stem).ok().map(|f| f.shared_by),
        ItemKind::Page => parse_page(bytes, stem).ok().and_then(|(_, by)| by),
        _ => None,
    }
}

// ---- the team workspace's files (Shared Context 2) ----

/// `<folder>/ordin-team-workspace.json` — at most 262,144 bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamWorkspaceFile {
    pub format: u8,
    pub workspace_key: String,
    pub name: String,
    pub name_changed_by: String,
    pub name_changed_at: Timestamp,
    pub places: Vec<TeamPlaceEntry>,
    pub agents: Vec<TeamAgentEntry>,
    pub results: TeamResultsEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamPlaceEntry {
    pub key: String,
    pub kind: String,
    pub location: String,
    pub display_name: String,
    pub changed_by: String,
    pub changed_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamAgentEntry {
    pub key: String,
    pub repository: AgentRepositoryRef,
    pub id: String,
    pub version: String,
    pub name: String,
    pub access: Vec<TeamAccessEntry>,
    pub changed_by: String,
    pub changed_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamAccessEntry {
    pub place: String,
    pub access: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamResultsEntry {
    pub kind: String,
    pub place: Option<String>,
    pub changed_by: String,
    pub changed_at: Timestamp,
}

/// `<folder>/activity/<id>.json` — at most 8,192 bytes; the file stem equals `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamActivityFile {
    pub format: u8,
    pub id: TeamActivityId,
    pub kind: String,
    pub by: String,
    pub at: Timestamp,
    pub task: Option<String>,
    pub agent: Option<String>,
    pub outcome: Option<String>,
    pub results: Vec<String>,
    pub change: Option<String>,
    pub subject: Option<String>,
    pub replaced_by: Option<String>,
}

/// The rule of any string of either file: no control character; lengths in Unicode scalar values
/// after NFC normalisation.
fn text_ok(s: &str, min: usize, max: usize) -> bool {
    !s.chars().any(char::is_control) && (min..=max).contains(&s.nfc().count())
}

pub fn login_ok(s: &str) -> bool {
    static LOGIN: OnceLock<Regex> = OnceLock::new();
    LOGIN
        .get_or_init(|| Regex::new(LOGIN_PATTERN).expect("pattern"))
        .is_match(s)
}

fn uuid_v7_text(s: &str) -> bool {
    uuid::Uuid::parse_str(s)
        .ok()
        .is_some_and(|u| u.get_version_num() == 7 && u.hyphenated().to_string() == s)
}

/// The four kinds a description names and their `SourceLinkKind`.
pub fn place_kind(kind: &str) -> Option<ordin_types::domain::SourceLinkKind> {
    use ordin_types::domain::SourceLinkKind as K;
    Some(match kind {
        "sharepoint" => K::SharepointLocation,
        "confluence" => K::ConfluenceLocation,
        "git_remote" => K::GitRemote,
        "jira_issue" => K::JiraIssue,
        _ => return None,
    })
}

pub fn place_kind_name(kind: ordin_types::domain::SourceLinkKind) -> Option<&'static str> {
    use ordin_types::domain::SourceLinkKind as K;
    Some(match kind {
        K::SharepointLocation => "sharepoint",
        K::ConfluenceLocation => "confluence",
        K::GitRemote => "git_remote",
        K::JiraIssue => "jira_issue",
        _ => return None,
    })
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("pattern"))
}

/// A location in its kind's canonical form, which admits no user part, query or token.
pub fn location_ok(kind: &str, location: &str) -> bool {
    static SP: OnceLock<Regex> = OnceLock::new();
    static CP: OnceLock<Regex> = OnceLock::new();
    static CS: OnceLock<Regex> = OnceLock::new();
    static JIRA: OnceLock<Regex> = OnceLock::new();
    if location.is_empty() || location.len() > 2_048 || location.chars().any(char::is_control) {
        return false;
    }
    match kind {
        "sharepoint" => {
            regex(&SP, r"^graph:drives/[^/?#@\s]+/items/[^/?#@\s]+$").is_match(location)
        }
        "confluence" => {
            regex(&CP, r"^confluence:page/[0-9]{1,64}$").is_match(location)
                || regex(&CS, r"^confluence:space/[A-Za-z0-9~._-]{1,255}$").is_match(location)
        }
        "jira_issue" => regex(
            &JIRA,
            r"^https://[^/?#@\s]+/browse/[A-Z][A-Z0-9_]{0,63}-[0-9]{1,12}$",
        )
        .is_match(location),
        "git_remote" => git_remote_ok(location),
        _ => false,
    }
}

/// `git:https://<host>/<owner>/<name>#<branch>:<path>` with the `GitRemote` rules of
/// WorkspaceService.md SA-CLOSE-03 and no `@`, `?` or second `#`.
fn git_remote_ok(location: &str) -> bool {
    let Some(rest) = location.strip_prefix("git:https://") else {
        return false;
    };
    if rest.contains('@') || rest.contains('?') || rest.matches('#').count() != 1 {
        return false;
    }
    let Some((address, tail)) = rest.split_once('#') else {
        return false;
    };
    let segments: Vec<&str> = address.split('/').collect();
    if segments.len() != 3
        || segments
            .iter()
            .any(|s| s.is_empty() || s.contains(char::is_whitespace))
    {
        return false;
    }
    let Some((branch, path)) = tail.split_once(':') else {
        return false;
    };
    (1..=255).contains(&branch.len())
        && path.len() <= 1_024
        && !path.starts_with('/')
        && !path.split('/').any(|s| s == "..")
}

/// An agent repository: `profile_key` 1–64 characters; `address` `https://` 1–2,048 bytes with no
/// user part and no query.
fn repository_ok(r: &AgentRepositoryRef) -> bool {
    match r {
        AgentRepositoryRef::ProfileKey(k) => text_ok(k, 1, 64),
        AgentRepositoryRef::Address(a) => {
            a.starts_with("https://")
                && (1..=2_048).contains(&a.len())
                && !a.contains('@')
                && !a.contains('?')
                && !a.chars().any(char::is_control)
        }
    }
}

// spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-TW-01
/// The description read strictly. `Err(None)` → `TEAM_WS_DESCRIPTION_UNREADABLE`; a parsed file of
/// a newer format → `Err(Some(format))` (`TEAM_WS_FORMAT_NEWER`).
pub fn parse_description(bytes: &[u8]) -> Result<TeamWorkspaceFile, Option<u8>> {
    if bytes.len() > DESCRIPTION_MAX_BYTES {
        return Err(None);
    }
    // The format is read first, so a newer description is told apart from an unreadable one.
    #[derive(Deserialize)]
    struct FormatOnly {
        format: u8,
    }
    let format = serde_json::from_slice::<FormatOnly>(bytes)
        .map_err(|_| None)?
        .format;
    if format > DESCRIPTION_FORMAT {
        return Err(Some(format));
    }
    let file: TeamWorkspaceFile = serde_json::from_slice(bytes).map_err(|_| None)?;
    if description_ok(&file) {
        Ok(file)
    } else {
        Err(None)
    }
}

pub fn description_ok(f: &TeamWorkspaceFile) -> bool {
    let place_keys: Vec<&str> = f.places.iter().map(|p| p.key.as_str()).collect();
    let mut unique = place_keys.clone();
    unique.sort_unstable();
    unique.dedup();
    if f.format != DESCRIPTION_FORMAT
        || !uuid_v7_text(&f.workspace_key)
        || !text_ok(&f.name, 1, 80)
        || !login_ok(&f.name_changed_by)
        || f.places.len() > MAX_PLACES
        || f.agents.len() > MAX_AGENTS
        || unique.len() != place_keys.len()
    {
        return false;
    }
    let places_ok = f.places.iter().all(|p| {
        uuid_v7_text(&p.key)
            && place_kind(&p.kind).is_some()
            && location_ok(&p.kind, &p.location)
            && text_ok(&p.display_name, 1, 120)
            && login_ok(&p.changed_by)
    });
    let mut agent_keys: Vec<&str> = f.agents.iter().map(|a| a.key.as_str()).collect();
    agent_keys.sort_unstable();
    let agent_count = agent_keys.len();
    agent_keys.dedup();
    let agents_ok = agent_keys.len() == agent_count
        && f.agents.iter().all(|a| {
            let mut access: Vec<&str> = a.access.iter().map(|e| e.place.as_str()).collect();
            access.sort_unstable();
            let n = access.len();
            access.dedup();
            uuid_v7_text(&a.key)
                && repository_ok(&a.repository)
                && text_ok(&a.id, 1, 300)
                && text_ok(&a.version, 1, 64)
                && text_ok(&a.name, 1, 120)
                && login_ok(&a.changed_by)
                && a.access.len() <= MAX_PLACES + 2
                && access.len() == n
                && a.access.iter().all(|e| {
                    (e.place == "own"
                        || e.place == "results"
                        || place_keys.contains(&e.place.as_str()))
                        && (e.access == "read" || e.access == "read_and_change")
                })
        });
    let results_ok = login_ok(&f.results.changed_by)
        && match (f.results.kind.as_str(), &f.results.place) {
            ("team_folder", None) => true,
            (kind @ ("sharepoint" | "confluence"), Some(key)) => {
                f.places.iter().any(|p| &p.key == key && p.kind == kind)
            }
            _ => false,
        };
    places_ok && agents_ok && results_ok
}

/// An activity file read strictly (Shared Context 2): `TEAM_FILE_UNREADABLE` at its path otherwise.
pub fn parse_activity(bytes: &[u8], stem: &str) -> Result<TeamActivityFile, ()> {
    if bytes.len() > ACTIVITY_MAX_BYTES {
        return Err(());
    }
    let f: TeamActivityFile = serde_json::from_slice(bytes).map_err(|_| ())?;
    if activity_ok(&f) && f.id.to_string() == stem {
        Ok(f)
    } else {
        Err(())
    }
}

pub fn activity_ok(f: &TeamActivityFile) -> bool {
    let opt = |v: &Option<String>, min, max| v.as_deref().is_none_or(|s| text_ok(s, min, max));
    if f.format != 1 || !login_ok(&f.by) {
        return false;
    }
    match f.kind.as_str() {
        "run" => {
            f.agent.as_deref().is_some_and(|a| text_ok(a, 1, 120))
                && f.outcome
                    .as_deref()
                    .is_some_and(|o| ["completed", "failed", "stopped"].contains(&o))
                && opt(&f.task, 1, 200)
                && f.change.is_none()
                && f.subject.is_none()
                && f.replaced_by.is_none()
                && f.results.len() <= 50
                && f.results.iter().all(|p| result_path_ok(p))
        }
        "replaced" => {
            f.change
                .as_deref()
                .is_some_and(|c| TEAM_CHANGES.contains(&c))
                && f.subject.as_deref().is_some_and(|s| text_ok(s, 1, 120))
                && f.replaced_by.as_deref().is_some_and(login_ok)
                && f.task.is_none()
                && f.agent.is_none()
                && f.outcome.is_none()
                && f.results.is_empty()
        }
        _ => false,
    }
}

// spec: ordin-desktop/components/TeamKnowledgeService#SA-TEAM-TW-13
/// The one result path rule (WorkspaceService.md WS-TW-9), the Workspace Service's own check.
pub fn result_path_ok(path: &str) -> bool {
    ordin_workspaces::check_result_path(path).is_ok()
}

/// SA-TEAM-TW-12: `<stem> (<login>)<ext>`, then ` (<login> 2)` to ` (<login> 99)`.
pub fn both_kept_name(path: &str, login: &str, n: u32) -> String {
    let (dir, file) = match path.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, path),
    };
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (file, String::new()),
    };
    let tag = if n <= 1 {
        login.to_owned()
    } else {
        format!("{login} {n}")
    };
    let name = format!("{stem} ({tag}){ext}");
    match dir {
        Some(d) => format!("{d}/{name}"),
        None => name,
    }
}
