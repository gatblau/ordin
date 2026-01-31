//! The Open Knowledge Format module (amendment January 2026, ADR-112; *Internal Logic* AC): one
//! reading and one rendering of OKF 0.2 concepts for this service, the Team Knowledge Service and
//! the Knowledge Reader. Pure functions: no I/O, no async, no clock.
//!
//! The header keeps ADR-104's split: owned keys are read with the restricted YAML reader and
//! rendered in a fixed order; every other top-level entry is kept as its original text
//! (SA-KLINK-01), and every source entry's other fields as their original lines (SA-OKF-04).

use std::fmt::Write as _;

use ordin_memory_service::PageSection;
use ordin_types::domain::places::KnowledgeAuthor;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use ordin_types::yaml::{read_restricted_node, YamlNode};
use ordin_types::KnowledgePageId;
use serde::Serialize;

use crate::markdown::{
    code_ranges, heading_of, in_ranges, lines_with_offsets, resolve_bundle_path, scan_links,
    split_sections, LinkForm,
};
use crate::names::{file_aliases, file_tags, name_key, path_stem};

pub const OKF_VERSION: &str = "0.2";
pub const PAGE_TYPES: [&str; 5] = ["Explanation", "How-to", "Decision", "Reference", "Glossary"];

/// The tier and status types are ordin-types' (2C wire), used by every crate.
pub use ordin_types::wire::knowledge_query::{PageStatus, PageTier};

/// 1 MiB + 64 KiB: the largest concept file read.
pub(crate) const CONCEPT_MAX_BYTES: usize = 1_048_576 + 65_536;
/// The restricted reader's bound on a header (the 1 MiB page bound).
pub(crate) const HEADER_MAX_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OkfLinkForm {
    Markdown,
    Wiki,
}

#[derive(Debug, Clone)]
pub struct OkfConcept {
    /// Bundle-relative path as given, with ".md", '/'-separated.
    pub path: String,
    /// 1–64 chars; None = missing or empty.
    pub type_: Option<String>,
    /// True when the header holds `ordin_knowledge_page: 1` (an ordin page).
    pub ordin_page: bool,
    /// The `title` key (1–200 chars), else the file stem; `title_written` says which.
    pub title: String,
    pub title_written: bool,
    /// As written, ≤ 2,000 chars kept.
    pub description: Option<String>,
    /// SA-KLINK-03 rules.
    pub tags: Vec<String>,
    /// ordin key; SA-KLINK-03 rules.
    pub aliases: Vec<String>,
    /// Body order, code excluded (§P step 1).
    pub links: Vec<OkfLink>,
    pub sources: Vec<OkfSource>,
    pub generated: Option<OkfGenerated>,
    pub verified: Vec<OkfVerification>,
    pub status: Option<PageStatus>,
    /// The value as written, kept for the round trip.
    pub status_text: Option<String>,
    /// None when absent or unreadable (SA-OKF-07).
    pub stale_after: Option<Timestamp>,
    /// §F step 2's split.
    pub sections: Vec<PageSection>,
    /// Ordin page keys when `ordin_page`.
    pub ordin: Option<OrdinPageKeys>,
    /// Every top-level entry not listed above, as its original text in order (SA-KLINK-01).
    pub extra_header: String,
    pub body: String,
}

// `PageSection` (the Memory Service's) has no `PartialEq`; sections compare by heading and text.
impl PartialEq for OkfConcept {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
            && self.type_ == other.type_
            && self.ordin_page == other.ordin_page
            && self.title == other.title
            && self.title_written == other.title_written
            && self.description == other.description
            && self.tags == other.tags
            && self.aliases == other.aliases
            && self.links == other.links
            && self.sources == other.sources
            && self.generated == other.generated
            && self.verified == other.verified
            && self.status == other.status
            && self.status_text == other.status_text
            && self.stale_after == other.stale_after
            && self.sections.len() == other.sections.len()
            && self
                .sections
                .iter()
                .zip(&other.sections)
                .all(|(a, b)| a.heading == b.heading && a.text == b.text)
            && self.ordin == other.ordin
            && self.extra_header == other.extra_header
            && self.body == other.body
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OkfLink {
    pub target: String,
    pub section: Option<String>,
    pub shown: Option<String>,
    pub form: OkfLinkForm,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OkfSource {
    pub origin: Option<String>,
    pub location: Option<String>,
    pub version_read: Option<String>,
    pub resource: Option<String>,
    pub id: Option<String>,
    pub last_modified: Option<String>,
    /// Other fields of the entry (title, author, unknown keys), as original lines (SA-OKF-04).
    pub extra: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OkfGenerated {
    pub by: String,
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OkfVerification {
    pub by: String,
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrdinPageKeys {
    pub page_id: KnowledgePageId,
    pub written_by: KnowledgeAuthor,
    pub last_changed: Timestamp,
    pub shared_by: Option<String>,
    pub shared_at: Option<Timestamp>,
}

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub path: String,
    pub title: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub at: Timestamp,
    pub by: String,
    pub created: Vec<IndexEntry>,
    pub updated: Vec<IndexEntry>,
    pub deprecated: Vec<IndexEntry>,
    pub removed: Vec<String>,
    /// Facts shared, as counts only: (new, changed, removed).
    pub facts: Option<(u32, u32, u32)>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OkfError {
    /// over 1 MiB + 64 KiB
    #[error("KNOW_OKF_TOO_LARGE")]
    TooLarge,
    #[error("KNOW_OKF_NOT_UTF8")]
    NotUtf8,
    /// front matter not YAML, refused by read_restricted, or an owned key of the wrong YAML type
    #[error("KNOW_OKF_HEADER_INVALID")]
    HeaderInvalid,
    /// index.md or log.md
    #[error("KNOW_OKF_RESERVED")]
    Reserved,
}

impl OkfError {
    /// The strings above; internal codes, never shown to a person.
    pub fn code(&self) -> &'static str {
        match self {
            OkfError::TooLarge => "KNOW_OKF_TOO_LARGE",
            OkfError::NotUtf8 => "KNOW_OKF_NOT_UTF8",
            OkfError::HeaderInvalid => "KNOW_OKF_HEADER_INVALID",
            OkfError::Reserved => "KNOW_OKF_RESERVED",
        }
    }
}

/// The keys this module owns at the top level of a header.
const OWNED_KEYS: [&str; 17] = [
    "ordin_knowledge_page",
    "page_id",
    "title",
    "type",
    "description",
    "written_by",
    "last_changed",
    "shared_by",
    "shared_at",
    "generated",
    "verified",
    "status",
    "stale_after",
    "sources",
    "tags",
    "aliases",
    // Never rendered by ordin; read as owned so it does not count as an extra entry twice.
    "okf_version",
];
const SOURCE_OWNED: [&str; 6] = [
    "origin",
    "location",
    "version_read",
    "resource",
    "id",
    "last_modified",
];

/// Splits a file into its front matter (between a first line `---` and the next line `---`) and
/// its body. `Ok(None)` when the file has no front matter.
pub(crate) fn split_front_matter(text: &str) -> Result<(Option<&str>, &str), OkfError> {
    let lines = lines_with_offsets(text);
    match lines.first() {
        Some((_, first)) if first.trim_end() == "---" => {}
        _ => return Ok((None, text)),
    }
    let header_start = text.find('\n').map(|i| i + 1).unwrap_or(text.len());
    for (start, line) in lines.iter().skip(1) {
        if line.trim_end() == "---" {
            let header = &text[header_start..*start];
            let after = start + line.len();
            let body_start = match text[after..].find('\n') {
                Some(i) => after + i + 1,
                None => text.len(),
            };
            return Ok((Some(header), &text[body_start..]));
        }
    }
    Err(OkfError::HeaderInvalid)
}

/// One top-level entry of a header: its key and its original text (with line ends).
#[derive(Debug, Clone)]
pub(crate) struct HeaderEntry {
    pub key: String,
    pub text: String,
}

/// SA-KLINK-01: the line `<key>:` at column 0 and every following line that is indented or
/// begins with `- `, up to the next column-0 line.
pub(crate) fn header_entries(header: &str) -> Vec<HeaderEntry> {
    let mut out: Vec<HeaderEntry> = Vec::new();
    for line in header.split_inclusive('\n') {
        let starts_entry = !line.trim().is_empty()
            && !line.starts_with(' ')
            && !line.starts_with('\t')
            && !line.starts_with("- ")
            && !line.starts_with('#')
            && line.trim_end() != "-";
        if starts_entry || out.is_empty() {
            out.push(HeaderEntry {
                key: if starts_entry {
                    key_of(line)
                } else {
                    String::new()
                },
                text: line.to_owned(),
            });
        } else if let Some(last) = out.last_mut() {
            last.text.push_str(line);
        }
    }
    out
}

fn key_of(line: &str) -> String {
    let t = line.trim_end();
    if let Some(q) = t.chars().next().filter(|c| *c == '"' || *c == '\'') {
        if let Some(end) = t[1..].find(q) {
            return t[1..1 + end].to_owned();
        }
    }
    t.split(':').next().unwrap_or("").trim().to_owned()
}

fn scalar(node: &YamlNode) -> Result<Option<String>, OkfError> {
    match node {
        YamlNode::Scalar { text, .. } => Ok(Some(text.clone())),
        YamlNode::Null => Ok(None),
        _ => Err(OkfError::HeaderInvalid),
    }
}

fn string_list(node: &YamlNode) -> Result<Vec<String>, OkfError> {
    match node {
        YamlNode::Seq(items) => Ok(items
            .iter()
            .filter_map(|i| match i {
                YamlNode::Scalar { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()),
        YamlNode::Scalar { text, .. } => Ok(vec![text.clone()]),
        YamlNode::Null => Ok(Vec::new()),
        YamlNode::Map(_) => Err(OkfError::HeaderInvalid),
    }
}

fn map_get<'a>(map: &'a [(String, YamlNode)], key: &str) -> Option<&'a YamlNode> {
    map.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// RFC 3339 date-time or a date (`YYYY-MM-DD`, start of that day UTC); anything else None.
pub(crate) fn parse_stale_after(raw: &str) -> Option<Timestamp> {
    use chrono::{NaiveDate, SubsecRound, TimeZone, Utc};
    let raw = raw.trim();
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(Timestamp(t.with_timezone(&Utc).trunc_subsecs(3)));
    }
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()?;
    let midnight = date.and_hms_opt(0, 0, 0)?;
    Some(Timestamp(Utc.from_utc_datetime(&midnight)))
}

/// `path` is the file's bundle-relative path with ".md"; `text` the whole file.
pub fn parse_concept(path: &str, text: &str) -> Result<OkfConcept, OkfError> {
    parse_counting(path, text).map(|(c, _)| c)
}

/// Values dropped while reading a header: (tags, aliases) (SA-KLINK-03).
pub(crate) type Dropped = (usize, usize);

/// `parse_concept`, also counting the tags and aliases dropped by SA-KLINK-03.
pub(crate) fn parse_counting(path: &str, text: &str) -> Result<(OkfConcept, Dropped), OkfError> {
    let mut dropped: Dropped = (0, 0);
    if is_reserved(path) {
        return Err(OkfError::Reserved);
    }
    if text.len() > CONCEPT_MAX_BYTES {
        return Err(OkfError::TooLarge);
    }
    let (header, body) = split_front_matter(text)?;
    let stem = path_stem(path);
    let mut concept = OkfConcept {
        path: path.to_owned(),
        type_: None,
        ordin_page: false,
        title: stem.clone(),
        title_written: false,
        description: None,
        tags: Vec::new(),
        aliases: Vec::new(),
        links: Vec::new(),
        sources: Vec::new(),
        generated: None,
        verified: Vec::new(),
        status: None,
        status_text: None,
        stale_after: None,
        sections: Vec::new(),
        ordin: None,
        extra_header: String::new(),
        body: body.to_owned(),
    };
    let mut timestamp_key: Option<String> = None;
    if let Some(header) = header {
        let tree =
            read_restricted_node(header, HEADER_MAX_BYTES).map_err(|_| OkfError::HeaderInvalid)?;
        let map = match tree {
            YamlNode::Map(m) => m,
            YamlNode::Null => Vec::new(),
            _ => return Err(OkfError::HeaderInvalid),
        };
        let entries = header_entries(header);
        // Owned keys.
        if let Some(n) = map_get(&map, "title") {
            if let Some(t) = scalar(n)? {
                let len = t.chars().count();
                if (1..=200).contains(&len) {
                    concept.title = t;
                    concept.title_written = true;
                }
            }
        }
        if let Some(n) = map_get(&map, "type") {
            concept.type_ = scalar(n)?
                .filter(|t| !t.trim().is_empty())
                .and_then(|t| crate::names::file_page_type(&t));
        }
        if let Some(n) = map_get(&map, "description") {
            concept.description = scalar(n)?.and_then(|d| crate::names::file_description(&d));
        }
        if let Some(n) = map_get(&map, "tags") {
            let (kept, d) = file_tags(&string_list(n)?);
            concept.tags = kept;
            dropped.0 = d;
        }
        if let Some(n) = map_get(&map, "aliases") {
            let (kept, d) = file_aliases(&string_list(n)?);
            concept.aliases = kept;
            dropped.1 = d;
        }
        if let Some(YamlNode::Map(g)) = map_get(&map, "generated") {
            if let Some(YamlNode::Scalar { text: by, .. }) = map_get(g, "by") {
                let at = map_get(g, "at").and_then(|a| scalar(a).ok().flatten());
                concept.generated = Some(OkfGenerated { by: by.clone(), at });
            }
        }
        if let Some(n) = map_get(&map, "verified") {
            match n {
                YamlNode::Seq(items) => {
                    for item in items {
                        if let YamlNode::Map(m) = item {
                            if let Some(YamlNode::Scalar { text: by, .. }) = map_get(m, "by") {
                                if !by.trim().is_empty() {
                                    let at =
                                        map_get(m, "at").and_then(|a| scalar(a).ok().flatten());
                                    concept
                                        .verified
                                        .push(OkfVerification { by: by.clone(), at });
                                }
                            }
                        }
                    }
                }
                YamlNode::Null => {}
                _ => return Err(OkfError::HeaderInvalid),
            }
        }
        if let Some(n) = map_get(&map, "status") {
            if let Some(s) = scalar(n)? {
                concept.status = Some(match s.as_str() {
                    "draft" => PageStatus::Draft,
                    "stable" => PageStatus::Stable,
                    "deprecated" => PageStatus::Deprecated,
                    _ => {
                        concept.status_text = Some(s.clone());
                        PageStatus::Stable
                    }
                });
            }
        }
        let mut stale_unreadable = false;
        if let Some(n) = map_get(&map, "stale_after") {
            if let Some(s) = scalar(n)? {
                concept.stale_after = parse_stale_after(&s);
                stale_unreadable = concept.stale_after.is_none();
            }
        }
        if let Some(n) = map_get(&map, "timestamp") {
            timestamp_key = scalar(n).ok().flatten();
        }
        // Sources: values from the tree, extra fields from the text.
        if let Some(n) = map_get(&map, "sources") {
            let items = match n {
                YamlNode::Seq(items) => items.clone(),
                YamlNode::Null => Vec::new(),
                _ => return Err(OkfError::HeaderInvalid),
            };
            let texts = entries
                .iter()
                .find(|e| e.key == "sources")
                .map(|e| source_item_texts(&e.text))
                .unwrap_or_default();
            for (i, item) in items.iter().enumerate() {
                let raw = texts.get(i).cloned().unwrap_or_default();
                match item {
                    YamlNode::Map(m) => {
                        let get = |k: &str| map_get(m, k).and_then(|v| scalar(v).ok().flatten());
                        concept.sources.push(OkfSource {
                            origin: get("origin"),
                            location: get("location"),
                            version_read: get("version_read"),
                            resource: get("resource"),
                            id: get("id"),
                            last_modified: get("last_modified"),
                            extra: source_extra(&raw),
                        });
                    }
                    _ => concept.sources.push(OkfSource {
                        extra: normalise_item(&raw),
                        ..OkfSource::default()
                    }),
                }
            }
        }
        // Ordin page keys.
        if let Some(n) = map_get(&map, "ordin_knowledge_page") {
            if scalar(n)?.as_deref() != Some("1") {
                return Err(OkfError::HeaderInvalid);
            }
            concept.ordin_page = true;
            concept.ordin = Some(ordin_keys(&map)?);
            if !concept.title_written || map_get(&map, "sources").is_none() {
                return Err(OkfError::HeaderInvalid);
            }
        }
        // Extra entries: every top-level entry whose key is not owned (an unreadable
        // `stale_after` is kept as written, SA-OKF-07).
        for e in &entries {
            let owned = OWNED_KEYS.contains(&e.key.as_str())
                && !(e.key == "stale_after" && stale_unreadable);
            if !owned {
                concept.extra_header.push_str(&e.text);
                if !concept.extra_header.ends_with('\n') {
                    concept.extra_header.push('\n');
                }
            }
        }
    }
    if concept.generated.is_none() {
        if let Some(at) = timestamp_key {
            // OKF 0.1: `timestamp` read as `generated.at` (the key stays an extra entry).
            concept.generated = Some(OkfGenerated {
                by: String::new(),
                at: Some(at),
            });
        }
    }
    concept
        .sources
        .extend(citations(&concept.body, &concept.sources));
    concept.links = concept_links(path, &concept.body);
    concept.sections = split_sections(&concept.title, &concept.body)
        .into_iter()
        .map(|(heading, text)| PageSection { heading, text })
        .collect();
    Ok((concept, dropped))
}

fn ordin_keys(map: &[(String, YamlNode)]) -> Result<OrdinPageKeys, OkfError> {
    let get = |k: &str| -> Result<Option<String>, OkfError> {
        match map_get(map, k) {
            Some(n) => scalar(n),
            None => Ok(None),
        }
    };
    let page_id = get("page_id")?
        .and_then(|p| KnowledgePageId::parse(&p).ok())
        .ok_or(OkfError::HeaderInvalid)?;
    let written_by = match get("written_by")?.as_deref() {
        Some("ordin") => KnowledgeAuthor::Ordin,
        Some("person") => KnowledgeAuthor::Person,
        _ => return Err(OkfError::HeaderInvalid),
    };
    let last_changed = get("last_changed")?
        .and_then(|t| parse_rfc3339_ms_utc(&t).ok())
        .map(Timestamp)
        .ok_or(OkfError::HeaderInvalid)?;
    let shared_by = get("shared_by")?;
    let shared_at = get("shared_at")?;
    let (shared_by, shared_at) = match (shared_by, shared_at) {
        (None, None) => (None, None),
        (Some(by), Some(at)) => {
            let n = by.chars().count();
            let at = parse_rfc3339_ms_utc(&at)
                .map(Timestamp)
                .map_err(|_| OkfError::HeaderInvalid)?;
            // An export of a page never shared carries `shared_by: ""` until the Team Knowledge
            // Service names the person (L-2); it reads as no sharer.
            if n == 0 {
                (None, Some(at))
            } else if n > 200 {
                return Err(OkfError::HeaderInvalid);
            } else {
                (Some(by), Some(at))
            }
        }
        _ => return Err(OkfError::HeaderInvalid),
    };
    Ok(OrdinPageKeys {
        page_id,
        written_by,
        last_changed,
        shared_by,
        shared_at,
    })
}

/// The original text of each list item of a `sources` entry (SA-OKF-04).
fn source_item_texts(entry: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut item_indent: Option<usize> = None;
    for line in entry.split_inclusive('\n').skip(1) {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let rest = &line[indent..];
        let is_item = rest.starts_with("- ") || rest.trim_end() == "-";
        if is_item && item_indent.is_none_or(|i| i == indent) {
            item_indent = Some(indent);
            items.push(line.to_owned());
        } else if let Some(last) = items.last_mut() {
            last.push_str(line);
        }
    }
    items
}

/// The lines of an item's fields that are not owned, re-indented to four spaces.
fn source_extra(item: &str) -> String {
    let mut out = String::new();
    let mut lines = item.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return out;
    };
    let item_indent = first.len() - first.trim_start_matches(' ').len();
    let field_indent = item_indent + 2;
    // The first field sits on the `- ` line.
    let mut fields: Vec<(String, Vec<String>)> = Vec::new();
    let first_field = first[item_indent..]
        .strip_prefix("- ")
        .unwrap_or("")
        .to_owned();
    if !first_field.trim().is_empty() {
        fields.push((
            key_of(&first_field),
            vec![format!("{}{first_field}", " ".repeat(4))],
        ));
    }
    for line in lines {
        let indent = line.len() - line.trim_start_matches(' ').len();
        let rest = &line[indent.min(line.len())..];
        if indent == field_indent && !rest.starts_with("- ") && !rest.trim().is_empty() {
            fields.push((key_of(rest), vec![format!("{}{rest}", " ".repeat(4))]));
        } else if let Some(last) = fields.last_mut() {
            let shifted = if indent >= field_indent {
                format!("{}{}", " ".repeat(4 + indent - field_indent), rest)
            } else {
                line.to_owned()
            };
            last.1.push(shifted);
        }
    }
    for (key, lines) in fields {
        if !SOURCE_OWNED.contains(&key.as_str()) {
            for l in lines {
                out.push_str(&l);
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn normalise_item(item: &str) -> String {
    let trimmed = item.trim_start_matches(' ');
    let mut out = format!("  {trimmed}");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// OKF 0.1: a body section headed `Citations` read as further sources.
fn citations(body: &str, existing: &[OkfSource]) -> Vec<OkfSource> {
    let code = code_ranges(body);
    let mut out: Vec<OkfSource> = Vec::new();
    let mut inside = false;
    for (start, line) in lines_with_offsets(body) {
        if in_ranges(&code, start) {
            continue;
        }
        if let Some(h) = heading_of(line) {
            inside = h.eq_ignore_ascii_case("citations");
            continue;
        }
        if !inside {
            continue;
        }
        let t = line.trim_start();
        let Some(item) = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .map(str::trim)
        else {
            continue;
        };
        if item.is_empty() {
            continue;
        }
        let resource = match (item.find("]("), item.rfind(')')) {
            (Some(open), Some(close)) if close > open + 2 => item[open + 2..close].to_owned(),
            _ => item.to_owned(),
        };
        let known = existing.iter().chain(out.iter()).any(|s| {
            s.resource.as_deref() == Some(resource.as_str())
                || s.location.as_deref() == Some(resource.as_str())
        });
        if !known {
            out.push(OkfSource {
                resource: Some(resource),
                ..OkfSource::default()
            });
        }
    }
    out
}

/// §P steps 1–3 over the body; a Markdown link path beginning `/` resolved from the bundle root.
fn concept_links(path: &str, body: &str) -> Vec<OkfLink> {
    let folder = crate::names::folder_of(path);
    scan_links(body)
        .into_iter()
        .filter_map(|l| {
            let target = match l.form {
                LinkForm::Markdown => resolve_bundle_path(folder, &l.target)?,
                _ => l.target.clone(),
            };
            Some(OkfLink {
                target,
                section: l.section,
                shown: l.shown,
                form: match l.form {
                    LinkForm::Markdown => OkfLinkForm::Markdown,
                    _ => OkfLinkForm::Wiki,
                },
            })
        })
        .collect()
}

/// SA-OKF-06.
pub fn trust_tier(concept: &OkfConcept) -> PageTier {
    tier_of(concept.verified.iter().map(|v| v.by.as_str()))
}

pub(crate) fn tier_of<'a>(by: impl Iterator<Item = &'a str>) -> PageTier {
    let mut any = false;
    for b in by {
        if b.trim().is_empty() {
            continue;
        }
        if b.starts_with("human:") {
            return PageTier::HumanReviewed;
        }
        any = true;
    }
    if any {
        PageTier::MachineConfirmed
    } else {
        PageTier::Unverified
    }
}

/// None or another value → Stable (SA-OKF-07).
pub fn effective_status(concept: &OkfConcept) -> PageStatus {
    concept.status.unwrap_or_default()
}

/// SA-OKF-07.
pub fn past_stale_after(concept: &OkfConcept, now: Timestamp) -> bool {
    concept.stale_after.is_some_and(|s| s <= now)
}

/// SA-OKF-08: `index.md` and `log.md`, compared case-insensitively, at any depth.
pub fn is_reserved(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name == "index.md" || name == "log.md"
}

/// SA-OKF-02: the body's first sentence, plain, cut at 200 characters on a word boundary with `…`.
pub fn first_sentence(body: &str) -> Option<String> {
    let body = match split_front_matter(body) {
        Ok((Some(_), rest)) => rest,
        _ => body,
    };
    let code = code_ranges(body);
    let mut paragraph = String::new();
    for (start, line) in lines_with_offsets(body) {
        let blank = line.trim().is_empty();
        if in_ranges(&code, start) || heading_of(line).is_some() || blank {
            if let Some(s) = sentence_in(&paragraph) {
                return Some(s);
            }
            paragraph.clear();
            continue;
        }
        let t = line
            .trim_start()
            .trim_start_matches(['>', ' '])
            .trim_start_matches("- ")
            .trim_start_matches("* ");
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(&plain_inline(t));
    }
    sentence_in(&paragraph)
}

fn sentence_in(paragraph: &str) -> Option<String> {
    let chars: Vec<char> = paragraph.trim().chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if matches!(c, '.' | '!' | '?') && chars.get(i + 1).is_none_or(|n| n.is_whitespace()) {
            let s: String = chars[..=i].iter().collect();
            let s = s.trim().to_owned();
            if s.is_empty() {
                return None;
            }
            return Some(cut_words(&s, crate::names::DESCRIPTION_MAX));
        }
    }
    None
}

fn cut_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let head: String = s.chars().take(max - 1).collect();
    let cut = match head.rfind(char::is_whitespace) {
        Some(i) if i > 0 => head[..i].trim_end().to_owned(),
        _ => head,
    };
    format!("{cut}…")
}

/// Markdown emphasis, link syntax and inline code markers removed.
fn plain_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("[[") {
            if let Some(end) = after.find("]]") {
                let inner = &after[..end];
                let shown = inner.split_once('|').map_or(inner, |(_, s)| s);
                out.push_str(shown.split('#').next().unwrap_or(shown));
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(end) = after.find("](") {
                if let Some(close) = after[end..].find(')') {
                    out.push_str(&after[..end]);
                    rest = &after[end + close + 1..];
                    continue;
                }
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        if !matches!(c, '*' | '_' | '`') {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Percent-encoding of SA-OKF-11: space, `%`, `(`, `)`, `<`, `>`, `[`, `]`, `#`, `?` and every
/// control or non-ASCII character (UTF-8 bytes).
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_control()
            || !b.is_ascii()
            || matches!(
                b,
                b' ' | b'%' | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'#' | b'?'
            )
        {
            let _ = write!(out, "%{b:02X}");
        } else {
            out.push(char::from(b));
        }
    }
    out
}

fn escape_link_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// SA-OKF-11: `[<shown>](/<path below the bundle root, percent-encoded>.md)`, with
/// `#<heading, percent-encoded>` for a section. `path` may carry its `.md` or not.
pub fn link_to(path: &str, section: Option<&str>, shown: &str) -> String {
    let path = crate::names::strip_md(path.trim_start_matches('/'));
    let mut out = format!("[{}](/{}.md", escape_link_text(shown), percent_encode(path));
    if let Some(s) = section {
        out.push('#');
        out.push_str(&percent_encode(s));
    }
    out.push(')');
    out
}

// ---------------------------------------------------------------- rendering

fn is_plain_safe(s: &str) -> bool {
    if s.is_empty() || s.trim() != s || s.chars().any(char::is_control) {
        return false;
    }
    if s.chars().any(|c| {
        matches!(
            c,
            ':' | '#'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | '&'
                | '*'
                | '!'
                | '|'
                | '>'
                | '\''
                | '"'
                | '%'
                | '@'
                | '`'
        )
    }) {
        return false;
    }
    if s.starts_with(['-', '?', '~']) {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "null" | "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n"
    ) {
        return false;
    }
    // A plain scalar the core schema would read as a number.
    let numeric = s
        .trim_start_matches(['+', '-'])
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '_' || c == 'e' || c == 'E')
        && s.chars().any(|c| c.is_ascii_digit());
    !numeric
        && !lower.starts_with("0x")
        && !lower.starts_with("0o")
        && lower != ".inf"
        && lower != ".nan"
}

/// A YAML double-quoted string, `\` and `"` escaped, control characters escaped.
pub(crate) fn yaml_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04X}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// AC-3's rule: double-quoted where the string holds a YAML indicator or begins or ends with
/// white space (and wherever a plain scalar would not read back as the same string), else plain.
pub(crate) fn yaml_str(s: &str) -> String {
    if is_plain_safe(s) {
        s.to_owned()
    } else {
        yaml_quoted(s)
    }
}

/// A 2C timestamp is written plain; any other value by the string rule.
fn yaml_time_or_str(s: &str) -> String {
    if parse_rfc3339_ms_utc(s).is_ok() {
        s.to_owned()
    } else {
        yaml_str(s)
    }
}

pub(crate) fn render_stale_after(t: Timestamp) -> String {
    use chrono::Timelike;
    let dt = t.0;
    if dt.hour() == 0 && dt.minute() == 0 && dt.second() == 0 && dt.nanosecond() == 0 {
        dt.format("%Y-%m-%d").to_string()
    } else {
        t.to_wire()
    }
}

fn flow_list(values: &[String]) -> String {
    let inner: Vec<String> = values.iter().map(|v| yaml_quoted(v)).collect();
    format!("[{}]", inner.join(", "))
}

fn status_word(s: PageStatus) -> &'static str {
    match s {
        PageStatus::Stable => "stable",
        PageStatus::Draft => "draft",
        PageStatus::Deprecated => "deprecated",
    }
}

/// Renders one source entry: owned fields in order, then its extra text unchanged.
// Owned fields are re-rendered in this module's form; `extra` is kept byte for byte (SA-OKF-04).
fn render_source(out: &mut String, s: &OkfSource) {
    let owned: [(&str, &Option<String>); 6] = [
        ("origin", &s.origin),
        ("location", &s.location),
        ("version_read", &s.version_read),
        ("resource", &s.resource),
        ("id", &s.id),
        ("last_modified", &s.last_modified),
    ];
    let present: Vec<(&str, &String)> = owned
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| (*k, v)))
        .collect();
    if present.is_empty() && s.extra.starts_with("  -") {
        out.push_str(&s.extra);
        return;
    }
    let mut first = true;
    for (k, v) in present {
        let value = match k {
            "origin" => yaml_str(v),
            "last_modified" => yaml_time_or_str(v),
            "version_read" | "id" => yaml_quoted(v),
            _ => yaml_str(v),
        };
        let lead = if first { "  - " } else { "    " };
        first = false;
        let _ = writeln!(out, "{lead}{k}: {value}");
    }
    for (i, line) in s.extra.split_inclusive('\n').enumerate() {
        if first && i == 0 {
            out.push_str("  - ");
            out.push_str(line.trim_start());
            first = false;
        } else {
            out.push_str(line);
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }
    if first {
        out.push_str("  - {}\n");
    }
}

/// Renders the whole file: owned ordin keys, then OKF keys, then extra entries, then the body.
pub fn render_concept(concept: &OkfConcept) -> String {
    let mut h = String::new();
    if let Some(o) = &concept.ordin {
        h.push_str("ordin_knowledge_page: 1\n");
        let _ = writeln!(h, "page_id: {}", o.page_id);
    }
    if concept.title_written || concept.ordin.is_some() {
        let _ = writeln!(h, "title: {}", yaml_str(&concept.title));
    }
    if let Some(t) = &concept.type_ {
        let _ = writeln!(h, "type: {}", yaml_str(t));
    }
    if let Some(d) = &concept.description {
        let _ = writeln!(h, "description: {}", yaml_quoted(d));
    }
    if let Some(o) = &concept.ordin {
        let by = match o.written_by {
            KnowledgeAuthor::Ordin => "ordin",
            KnowledgeAuthor::Person => "person",
        };
        let _ = writeln!(h, "written_by: {by}");
        let _ = writeln!(h, "last_changed: {}", o.last_changed);
        // Rule K: a page that was never shared renders neither key.
        if let (Some(by), Some(at)) = (&o.shared_by, &o.shared_at) {
            let _ = writeln!(h, "shared_by: {}", yaml_quoted(by));
            let _ = writeln!(h, "shared_at: {at}");
        }
    }
    if let Some(g) = &concept.generated {
        if !g.by.is_empty() {
            h.push_str("generated:\n");
            let _ = writeln!(h, "  by: {}", yaml_quoted(&g.by));
            if let Some(at) = &g.at {
                let _ = writeln!(h, "  at: {}", yaml_time_or_str(at));
            }
        }
    }
    if !concept.verified.is_empty() {
        h.push_str("verified:\n");
        for v in &concept.verified {
            let _ = writeln!(h, "  - by: {}", yaml_quoted(&v.by));
            if let Some(at) = &v.at {
                let _ = writeln!(h, "    at: {}", yaml_time_or_str(at));
            }
        }
    }
    if let Some(text) = &concept.status_text {
        let _ = writeln!(h, "status: {}", yaml_str(text));
    } else if let Some(s) = concept.status {
        let _ = writeln!(h, "status: {}", status_word(s));
    }
    if let Some(s) = concept.stale_after {
        let _ = writeln!(h, "stale_after: {}", render_stale_after(s));
    }
    if !concept.sources.is_empty() {
        h.push_str("sources:\n");
        for s in &concept.sources {
            render_source(&mut h, s);
        }
    } else if concept.ordin.is_some() {
        h.push_str("sources: []\n");
    }
    if !concept.tags.is_empty() {
        let _ = writeln!(h, "tags: {}", flow_list(&concept.tags));
    }
    if !concept.aliases.is_empty() {
        let _ = writeln!(h, "aliases: {}", flow_list(&concept.aliases));
    }
    h.push_str(&concept.extra_header);
    if !h.is_empty() && !h.ends_with('\n') {
        h.push('\n');
    }
    format!("---\n{h}---\n{}", concept.body)
}

/// The workspace index (AC-4).
pub fn render_index(entries: &[IndexEntry], generated_by: &str, at: Timestamp) -> String {
    let mut sorted: Vec<&IndexEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        name_key(&a.title)
            .cmp(&name_key(&b.title))
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut out = String::new();
    let _ = write!(
        out,
        "---\nokf_version: \"{OKF_VERSION}\"\ngenerated:\n  by: {}\n  at: {at}\n---\n# Knowledge\n\n",
        yaml_quoted(generated_by)
    );
    for e in sorted {
        out.push_str("- ");
        out.push_str(&link_to(&e.path, None, &e.title));
        if let Some(d) = &e.description {
            out.push_str(" — ");
            out.push_str(&escape_link_text(d));
        }
        out.push('\n');
    }
    out
}

/// `existing` = the current log.md text (None = no file); returns the whole new text (AC-5).
pub fn append_log(existing: Option<&str>, entry: &LogEntry) -> String {
    let mut out = existing.unwrap_or("# Log\n").to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    let _ = writeln!(out, "## {} — {}", entry.at.0.format("%Y-%m-%d"), entry.by);
    let links = |items: &[IndexEntry]| -> String {
        items
            .iter()
            .map(|e| link_to(&e.path, None, &e.title))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !entry.created.is_empty() {
        let _ = writeln!(out, "- Created: {}", links(&entry.created));
    }
    if !entry.updated.is_empty() {
        let _ = writeln!(out, "- Updated: {}", links(&entry.updated));
    }
    if !entry.deprecated.is_empty() {
        let _ = writeln!(out, "- Deprecated: {}", links(&entry.deprecated));
    }
    if !entry.removed.is_empty() {
        let _ = writeln!(out, "- Removed: {}", entry.removed.join(", "));
    }
    if let Some((n, c, r)) = entry.facts {
        let _ = writeln!(out, "- Facts: {n} new, {c} changed, {r} removed");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "---\nordin_knowledge_page: 1\npage_id: 0192b0a1-1111-7222-8333-444455556666\ntitle: How supplier rates are set\ntype: Explanation\ndescription: \"How supplier day rates are set and reviewed each quarter.\"\nwritten_by: ordin\nlast_changed: 2026-01-10T14:03:22.410Z\ngenerated:\n  by: \"ordin/0.61.0\"\n  at: 2026-01-10T14:03:22.410Z\nverified:\n  - by: \"human:asha-patel\"\n    at: 2026-01-10T14:10:05.002Z\nsources:\n  - origin: place\n    location: \"place:0192a0f2-2222-7333-8444-555566667777:Suppliers/Rate card.xlsx\"\n    version_read: \"7\"\n    resource: \"place:0192a0f2-2222-7333-8444-555566667777:Suppliers/Rate card.xlsx\"\n    id: \"3f2a9c41d07be815\"\n    last_modified: 2026-01-10T14:03:22.410Z\n    title: \"Rate card\"\ntags: [\"suppliers\"]\naliases: [\"Rate rules\"]\ncssclasses: [wide]\n---\n## Review\nRates are reviewed each quarter against the [purchasing policy](/Purchasing%20policy.md).\n";

    #[test]
    fn the_example_round_trips_byte_for_byte() {
        let c = parse_concept("How supplier rates are set.md", EXAMPLE).unwrap();
        assert!(c.ordin_page);
        assert_eq!(c.type_.as_deref(), Some("Explanation"));
        assert_eq!(c.tags, vec!["suppliers"]);
        assert_eq!(c.sources[0].extra, "    title: \"Rate card\"\n");
        assert_eq!(c.extra_header, "cssclasses: [wide]\n");
        assert_eq!(trust_tier(&c), PageTier::HumanReviewed);
        assert_eq!(c.links[0].target, "Purchasing policy");
        assert_eq!(render_concept(&c), EXAMPLE);
    }

    #[test]
    fn reserved_names_and_errors() {
        assert_eq!(parse_concept("a/INDEX.md", "x"), Err(OkfError::Reserved));
        assert!(is_reserved("log.md"));
        assert_eq!(
            parse_concept("a.md", "---\nx: &a 1\ny: *a\n---\n"),
            Err(OkfError::HeaderInvalid)
        );
        assert_eq!(
            parse_concept("a.md", "---\ntitle: [1, 2]\n---\n"),
            Err(OkfError::HeaderInvalid)
        );
        assert_eq!(
            parse_concept("a.md", &"x".repeat(CONCEPT_MAX_BYTES + 1)),
            Err(OkfError::TooLarge)
        );
    }

    #[test]
    fn a_foreign_concept_is_read() {
        let text = "---\ntype: Supplier Profile\nowner_team: buying\nstatus: retired\nstale_after: 2026-01-10\n---\nSee [Packaging](/missing.md).\n";
        let c = parse_concept("suppliers/borealis.md", text).unwrap();
        assert_eq!(c.title, "borealis");
        assert!(!c.title_written);
        assert_eq!(c.type_.as_deref(), Some("Supplier Profile"));
        assert_eq!(c.links[0].target, "missing");
        assert_eq!(effective_status(&c), PageStatus::Stable);
        assert_eq!(c.status_text.as_deref(), Some("retired"));
        assert!(past_stale_after(&c, Timestamp::now()));
        assert_eq!(c.extra_header, "owner_team: buying\n");
        assert!(render_concept(&c).contains("stale_after: 2026-01-10\n"));
    }

    #[test]
    fn first_sentences_and_links() {
        assert_eq!(
            first_sentence("# T\n\nThe **rates** are [set](/a.md) here. More.").as_deref(),
            Some("The rates are set here.")
        );
        assert_eq!(first_sentence("no end"), None);
        assert_eq!(
            link_to("Supplier rates", Some("Day rates"), "rates"),
            "[rates](/Supplier%20rates.md#Day%20rates)"
        );
    }

    #[test]
    fn index_and_log() {
        let at = Timestamp::now();
        let idx = render_index(
            &[
                IndexEntry {
                    path: "Supplier rates.md".into(),
                    title: "Supplier rates".into(),
                    description: None,
                },
                IndexEntry {
                    path: "Borealis.md".into(),
                    title: "Borealis".into(),
                    description: Some("Our packaging supplier.".into()),
                },
            ],
            "ordin/1.0.0",
            at,
        );
        assert!(idx.contains("okf_version: \"0.2\""));
        assert!(idx
            .contains("- [Borealis](/Borealis.md) — Our packaging supplier.\n- [Supplier rates]"));
        let log = append_log(
            None,
            &LogEntry {
                at,
                by: "asha".into(),
                created: vec![],
                updated: vec![],
                deprecated: vec![],
                removed: vec!["Old".into()],
                facts: Some((1, 2, 3)),
            },
        );
        assert!(log.starts_with("# Log\n\n## "));
        assert!(log.ends_with("- Removed: Old\n- Facts: 1 new, 2 changed, 3 removed\n"));
    }

    #[test]
    fn quoting_follows_the_rule() {
        assert_eq!(yaml_str("plain words"), "plain words");
        assert_eq!(yaml_str("a: b"), "\"a: b\"");
        assert_eq!(yaml_str("7"), "\"7\"");
        assert_eq!(yaml_str("true"), "\"true\"");
        assert_eq!(yaml_quoted("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
