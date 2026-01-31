//! The withdrawn list file (CatalogueService.md §E): parsing and the `WDL_` rules. One invalid
//! entry makes the whole list unreadable (fail closed, R-78).

use ordin_agent_model::{parse_withdrawn_list, PartKind, WITHDRAWN_LIST_SCHEMA};
use ordin_types::num::Sha256Hex;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};

/// One validated entry of `withdrawn.yaml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileEntry {
    pub kind: PartKind,
    pub identifier: String,
    pub version: Option<String>,
    pub digest: Option<Sha256Hex>,
    pub reason: String,
    pub withdrawn_at: Timestamp,
    pub replacement: Option<(String, String)>,
}

impl FileEntry {
    /// The identity of an entry: kind, identifier and the version or none.
    pub(crate) fn key(&self) -> (PartKind, String, String) {
        (
            self.kind,
            self.identifier.clone(),
            self.version.clone().unwrap_or_default(),
        )
    }
}

/// Parses and validates the file; the error is the `WDL_` or `AM_` code that made it unreadable.
// spec: ordin-desktop/components/CatalogueService#ADR-091
pub(crate) fn read_list(bytes: &[u8]) -> Result<Vec<FileEntry>, &'static str> {
    let file = parse_withdrawn_list(bytes).map_err(|e| e.code())?;
    if file.list_schema != WITHDRAWN_LIST_SCHEMA {
        return Err("AM_WITHDRAWN_LIST_INVALID");
    }
    let mut entries = Vec::with_capacity(file.entries.len());
    for entry in file.entries {
        let reason_chars = entry.reason.chars().count();
        if reason_chars == 0 || reason_chars > 1000 {
            return Err("WDL_REASON_INVALID");
        }
        if entry.part.version.is_some() != entry.part.content_hash.is_some() {
            return Err("WDL_VERSION_DIGEST_MISMATCH");
        }
        let digest = match &entry.part.content_hash {
            Some(hash) => Some(Sha256Hex::parse(hash).map_err(|_| "WDL_VERSION_DIGEST_MISMATCH")?),
            None => None,
        };
        let replacement = match entry.replacement {
            None => None,
            Some(r) => match r.version {
                Some(version) => Some((r.id, version)),
                None => return Err("WDL_REPLACEMENT_INCOMPLETE"),
            },
        };
        let withdrawn_at = parse_rfc3339_ms_utc(&entry.withdrawn_at)
            .map(Timestamp)
            .map_err(|_| "AM_WITHDRAWN_LIST_INVALID")?;
        entries.push(FileEntry {
            kind: entry.part.kind,
            identifier: entry.part.id,
            version: entry.part.version,
            digest,
            reason: entry.reason,
            withdrawn_at,
            replacement,
        });
    }
    Ok(entries)
}
