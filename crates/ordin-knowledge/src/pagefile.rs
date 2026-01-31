//! A page file and its row: B-5's rendering (with K, the ADR-104 header split and ADR-112's OKF
//! keys) and D-1's reading of a file back into row values.

use ordin_types::domain::places::KnowledgeAuthor;
use ordin_types::time::Timestamp;
use ordin_types::wire::knowledge_query::{PageStatus, PageTier};
use ordin_types::KnowledgePageId;
use sha2::{Digest, Sha256};

use crate::error::KnowledgeError;
use crate::names::below_knowledge;
use crate::okf::{self, OkfConcept, OkfGenerated, OkfSource, OkfVerification, OrdinPageKeys};
use crate::store::{Generated, PageRow, StoredSource, Verification};

/// At most 64 KiB of extra header entries (SA-KLINK-01).
pub(crate) const EXTRA_HEADER_MAX: usize = 65_536;

/// SA-OKF-05: the first 16 hexadecimal characters of SHA-256 of `origin + "\n" + location`.
// spec: ordin-desktop/components/KnowledgeService#SA-OKF-05
pub(crate) fn source_id(origin: &str, location: &str) -> String {
    let digest = Sha256::digest(format!("{origin}\n{location}").as_bytes());
    let mut out = String::with_capacity(16);
    for b in digest.iter().take(8) {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// The bundle-relative path of an own page's file (`knowledge/` is the bundle root).
pub(crate) fn concept_path(relative_path: &str) -> String {
    format!("{}.md", below_knowledge(relative_path))
}

fn okf_source(s: &StoredSource, page_last_changed: Timestamp) -> OkfSource {
    let extra = s.extra.clone().unwrap_or_default();
    if s.origin == "okf" {
        // Another producer's entry: only what it wrote (SA-OKF-05).
        return OkfSource {
            resource: Some(s.resource.clone().unwrap_or_else(|| s.location.clone())),
            id: s.id.clone(),
            last_modified: s.last_modified.clone(),
            extra,
            ..OkfSource::default()
        };
    }
    OkfSource {
        origin: Some(s.origin.clone()),
        location: Some(s.location.clone()),
        version_read: Some(s.version_read.clone()),
        resource: Some(s.location.clone()),
        id: Some(source_id(&s.origin, &s.location)),
        last_modified: Some(
            s.last_modified
                .clone()
                .unwrap_or_else(|| page_last_changed.to_wire()),
        ),
        extra,
    }
}

/// The concept of an own page as B-5 renders it.
pub(crate) fn concept_of(row: &PageRow) -> OkfConcept {
    OkfConcept {
        path: concept_path(&row.relative_path),
        type_: row.page_type.clone(),
        ordin_page: true,
        title: row.title.clone(),
        title_written: true,
        description: row.description.clone(),
        tags: row.tags.clone(),
        aliases: row.aliases.clone(),
        links: Vec::new(),
        sources: row
            .sources
            .iter()
            .map(|s| okf_source(s, row.last_changed_at))
            .collect(),
        generated: row.generated.as_ref().map(|g| OkfGenerated {
            by: g.by.clone(),
            at: g.at.clone(),
        }),
        verified: row
            .verified
            .iter()
            .map(|v| OkfVerification {
                by: v.by.clone(),
                at: v.at.clone(),
            })
            .collect(),
        status: (row.okf_status != PageStatus::Stable).then_some(row.okf_status),
        status_text: row.status_text.clone(),
        stale_after: row.stale_after.as_deref().and_then(okf::parse_stale_after),
        sections: Vec::new(),
        ordin: Some(OrdinPageKeys {
            page_id: row.id,
            written_by: row.written_by,
            last_changed: row.last_changed_at,
            shared_by: row.shared_by.clone(),
            shared_at: row.shared_at,
        }),
        extra_header: row.extra_header.clone(),
        body: row.body.clone(),
    }
}

pub(crate) fn render(row: &PageRow) -> Vec<u8> {
    okf::render_concept(&concept_of(row)).into_bytes()
}

/// What a page file's header says, read by D-1 (as amended by ADR-104 and ADR-112).
#[derive(Debug, Clone)]
pub(crate) struct FileHeader {
    pub ordin_page: bool,
    pub page_id: Option<KnowledgePageId>,
    pub title: String,
    pub title_written: bool,
    pub written_by: Option<KnowledgeAuthor>,
    pub last_changed: Option<Timestamp>,
    pub shared_by: Option<String>,
    pub shared_at: Option<Timestamp>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub dropped_tags: usize,
    pub dropped_aliases: usize,
    pub extra_header: String,
    pub sources: Vec<StoredSource>,
    pub page_type: Option<String>,
    pub description: Option<String>,
    pub verified: Vec<Verification>,
    pub generated: Option<Generated>,
    pub tier: PageTier,
    pub status: PageStatus,
    pub status_text: Option<String>,
    pub stale_after: Option<String>,
    pub body: String,
}

fn stored_sources(sources: &[OkfSource]) -> Vec<StoredSource> {
    sources
        .iter()
        .filter_map(|s| match (&s.origin, &s.location, &s.version_read) {
            (Some(o), Some(l), Some(v)) => Some(StoredSource {
                origin: o.clone(),
                location: l.clone(),
                version_read: v.clone(),
                resource: s.resource.clone(),
                id: s.id.clone(),
                last_modified: s.last_modified.clone(),
                extra: (!s.extra.is_empty()).then(|| s.extra.clone()),
            }),
            _ => s.resource.as_ref().map(|r| StoredSource {
                origin: "okf".to_owned(),
                location: r.clone(),
                version_read: s
                    .last_modified
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
                resource: Some(r.clone()),
                id: s.id.clone(),
                last_modified: s.last_modified.clone(),
                extra: (!s.extra.is_empty()).then(|| s.extra.clone()),
            }),
        })
        .collect()
}

/// Reads a file by `okf::parse_concept`. `path` is the bundle-relative concept path.
pub(crate) fn read_concept_file(path: &str, text: &str) -> Result<FileHeader, okf::OkfError> {
    let (c, (dropped_tags, dropped_aliases)) = okf::parse_counting(path, text)?;
    let tier = okf::trust_tier(&c);
    Ok(FileHeader {
        ordin_page: c.ordin_page,
        page_id: c.ordin.as_ref().map(|o| o.page_id),
        title: c.title.clone(),
        title_written: c.title_written,
        written_by: c.ordin.as_ref().map(|o| o.written_by),
        last_changed: c.ordin.as_ref().map(|o| o.last_changed),
        shared_by: c.ordin.as_ref().and_then(|o| o.shared_by.clone()),
        shared_at: c.ordin.as_ref().and_then(|o| o.shared_at),
        tags: c.tags.clone(),
        aliases: c.aliases.clone(),
        dropped_tags,
        dropped_aliases,
        extra_header: c.extra_header.clone(),
        sources: stored_sources(&c.sources),
        page_type: c.type_.clone(),
        description: c.description.clone(),
        verified: c
            .verified
            .iter()
            .map(|v| Verification {
                by: v.by.clone(),
                at: v.at.clone(),
            })
            .collect(),
        generated: c.generated.as_ref().map(|g| Generated {
            by: g.by.clone(),
            at: g.at.clone(),
        }),
        tier,
        status: okf::effective_status(&c),
        status_text: c.status_text.clone(),
        stale_after: c.stale_after.map(okf::render_stale_after),
        body: c.body,
    })
}

/// D-1 for an own page file: the ordin keys required, extra entries at most 64 KiB.
pub(crate) fn read_page_file(
    relative_path: &str,
    bytes: &[u8],
) -> Result<FileHeader, KnowledgeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| KnowledgeError::HeaderInvalid)?;
    let h = read_concept_file(&concept_path(relative_path), text)
        .map_err(|_| KnowledgeError::HeaderInvalid)?;
    if !h.ordin_page || h.extra_header.len() > EXTRA_HEADER_MAX {
        return Err(KnowledgeError::HeaderInvalid);
    }
    Ok(h)
}
