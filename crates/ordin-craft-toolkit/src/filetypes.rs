//! The file-type map (CraftToolkit.md *Internal Logic* 3).

use std::collections::{BTreeMap, BTreeSet};

use ordin_files::media_type;
use ordin_types::domain::engineering::FileTypeMapEntry;
use ordin_types::FileTypeMapEntryId;

use crate::error::CraftError;
use crate::model::CraftProfile;
use crate::store;
use crate::toolkit::CraftToolkit;

/// `[a-z0-9]{1,16}` without the dot.
fn extension_ok(extension: &str) -> bool {
    (1..=16).contains(&extension.len())
        && extension
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// RFC 6838 section 4.2: a restricted name, 1–127 characters.
fn restricted_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let first = bytes.next().is_some_and(|b| b.is_ascii_alphanumeric());
    first
        && name.len() <= 127
        && bytes.all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
                )
        })
}

fn media_type_ok(media: &str) -> bool {
    media
        .split_once('/')
        .is_some_and(|(top, sub)| restricted_name(top) && restricted_name(sub))
}

/// The 2C rules of one entry.
pub(crate) fn check_entry(entry: &FileTypeMapEntry) -> Result<(), CraftError> {
    if entry.extensions.is_empty() || !entry.extensions.iter().all(|e| extension_ok(e)) {
        return Err(CraftError::Invalid("FTM_EXTENSION_INVALID"));
    }
    if !entry.media_types.iter().all(|m| media_type_ok(m)) {
        return Err(CraftError::Invalid("FTM_MEDIA_TYPE_INVALID"));
    }
    Ok(())
}

fn skipped(code: &'static str) {
    tracing::warn!(component = "craft", code, "craft_extension_skipped");
}

impl CraftToolkit {
    /// Internal Logic 3.1 and 3.2.
    pub(crate) fn map_entries(&self, profile: &CraftProfile) -> Vec<FileTypeMapEntry> {
        let approved = |app: &str| profile.apps.iter().any(|pin| pin.name == app);
        let mut groups: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for (raw, app) in &profile.file_types {
            let extension = raw.trim_start_matches('.').to_lowercase();
            if !extension_ok(&extension) {
                skipped("FTM_EXTENSION_INVALID");
            } else if !seen.insert(extension.clone()) {
                skipped("FTM_EXTENSION_DUPLICATE");
            } else {
                let app = approved(app).then(|| app.clone());
                groups.entry(app).or_default().push(extension);
            }
        }
        let mut entries = Vec::with_capacity(groups.len());
        for (open_with_app, extensions) in groups {
            let mut media_types: Vec<String> = extensions
                .iter()
                .map(|e| media_type(e).to_owned())
                .collect();
            media_types.dedup();
            let entry = FileTypeMapEntry {
                id: FileTypeMapEntryId::new(),
                craft_tools: self.tools_of(profile, open_with_app.as_deref()),
                extensions,
                media_types,
                open_with_app,
                profile_version: profile.profile_version.clone(),
            };
            match check_entry(&entry) {
                Ok(()) => entries.push(entry),
                Err(error) => skipped(error.code()),
            }
        }
        entries
    }

    /// The app's `allowed_tools` that exist in its definition; every tool of its definition when the
    /// list is `None`.
    fn tools_of(&self, profile: &CraftProfile, app: Option<&str>) -> Vec<String> {
        let Some(pin) = app.and_then(|a| profile.apps.iter().find(|p| p.name == a)) else {
            return Vec::new();
        };
        let Some(definition) = self.definition(&pin.name, &pin.version) else {
            return Vec::new();
        };
        definition
            .tools
            .iter()
            .map(|tool| tool.tool.clone())
            .filter(|tool| allowed_by(pin.allowed_tools.as_deref(), tool))
            .collect()
    }

    /// Internal Logic 3.3. A row is changed only by a new profile: the rows of the same version
    /// stay, and a different map under that version is `FTM_IMMUTABLE`.
    pub(crate) async fn apply_file_types(&self, profile: &CraftProfile) -> Result<(), CraftError> {
        let stored = store::load_entries(&self.pool).await?;
        let entries = self.map_entries(profile);
        if stored
            .first()
            .is_some_and(|e| e.profile_version == profile.profile_version)
        {
            if shape_of(&stored) != shape_of(&entries) {
                return Err(CraftError::Invalid("FTM_IMMUTABLE"));
            }
            self.set_file_types(stored);
            return Ok(());
        }
        store::replace_entries(&self.pool, &entries).await?;
        self.set_file_types(entries);
        Ok(())
    }

    pub fn file_kind(&self, extension: &str) -> Option<FileTypeMapEntry> {
        let extension = extension.trim_start_matches('.').to_lowercase();
        crate::toolkit::lock(&self.state)
            .file_types
            .get(&extension)
            .cloned()
    }
}

/// What the profile decides: each extension's app. `craft_tools` follow the shipped definitions,
/// which change with an ordin release, so they are not part of the rule.
fn shape_of(entries: &[FileTypeMapEntry]) -> BTreeMap<String, Option<String>> {
    entries
        .iter()
        .flat_map(|e| {
            e.extensions
                .iter()
                .map(|x| (x.clone(), e.open_with_app.clone()))
        })
        .collect()
}

/// An absent or empty `allowed_tools` means every tool (SA-CLOSE-01).
pub(crate) fn allowed_by(allowed: Option<&[String]>, tool: &str) -> bool {
    match allowed {
        None | Some([]) => true,
        Some(list) => list.iter().any(|t| t == tool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_names_follow_rfc_6838() {
        for good in ["image/png", "application/vnd.ms-excel", "image/svg+xml"] {
            assert!(media_type_ok(good), "{good}");
        }
        for bad in ["png", "/png", "image/", "image/ png", "-image/png"] {
            assert!(!media_type_ok(bad), "{bad}");
        }
    }

    #[test]
    fn an_empty_allowed_list_means_every_tool() {
        let list = ["resize".to_owned()];
        assert!(allowed_by(None, "x"));
        assert!(allowed_by(Some(&[]), "x"));
        assert!(allowed_by(Some(&list), "resize"));
        assert!(!allowed_by(Some(&list), "x"));
    }
}
