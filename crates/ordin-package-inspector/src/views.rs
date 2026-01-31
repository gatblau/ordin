//! Steps 9 to 11: the screens' views of a stored model, the installed files and a part's page.

use std::path::Path;

use ordin_agent_model::{kind_info, markdown_body, PartKind, PartRef};

use crate::error::InspectError;
use crate::fsio::{primary_manifest, sha256_hex, strip_controls_keep_lines, walk, EntryKind};
use crate::model::{
    Detail, InstalledFileEntry, InstalledFileText, InstalledMadeOf, KindGroup, PackageModel,
    PartLine, PartPage,
};

/// The bytes of a page, cut on a character boundary (A-45).
const MAX_PAGE_BYTES: usize = 100 * 1024;

/// The order of 11:973, each group as the kinds it holds. `LinksCode` adds the code workflow to the
/// language, writing-style and hand-over group.
fn group_kinds(detail: Detail) -> Vec<Vec<PartKind>> {
    let mut overlays = vec![
        PartKind::LanguageOverlay,
        PartKind::StyleOverlay,
        PartKind::HandoffOverlay,
    ];
    if detail == Detail::LinksCode {
        overlays.push(PartKind::SourceWorkflowOverlay);
    }
    vec![
        vec![PartKind::SystemRole],
        vec![PartKind::Rule],
        vec![PartKind::OperatingMode],
        vec![PartKind::Skill, PartKind::Capability],
        vec![PartKind::Command],
        vec![PartKind::ContextResource],
        vec![PartKind::ToolBinding],
        vec![PartKind::Memory],
        vec![PartKind::ControlLoop],
        vec![PartKind::PracticePack],
        overlays,
        vec![PartKind::Evaluation],
        vec![PartKind::OutputContract],
    ]
}

/// A business user sees a part's kind and name only: every technical field is cleared to an empty
/// string (11 section 5.29 *Never shown*).
pub(crate) fn business_line(line: &PartLine) -> PartLine {
    PartLine {
        part: PartRef {
            kind: line.part.kind,
            id: String::new(),
            name: line.part.name.clone(),
            version: String::new(),
            content_hash: String::new(),
            repository: String::new(),
            commit: String::new(),
            loading: None,
        },
        ..line.clone()
    }
}

/// Step 9.
pub(crate) fn made_of(model: &PackageModel, detail: Detail) -> InstalledMadeOf {
    let mut groups = Vec::new();
    let mut shown: u32 = 0;
    for kinds in group_kinds(detail) {
        let mut parts: Vec<PartLine> = model
            .parts
            .iter()
            .filter(|line| kinds.contains(&line.part.kind))
            .cloned()
            .collect();
        if parts.is_empty() {
            continue;
        }
        parts.sort_by(|a, b| {
            a.name
                .as_bytes()
                .cmp(b.name.as_bytes())
                .then_with(|| a.part.id.cmp(&b.part.id))
        });
        // One group holds several kinds (abilities; language, writing style and hand-over), and
        // `plain_kind_message_id` is one id: the id of the first kind in the group
        // order that has a part is used.
        let first_kind = kinds
            .iter()
            .find(|kind| parts.iter().any(|line| line.part.kind == **kind))
            .copied()
            .unwrap_or(kinds[0]);
        if detail == Detail::BusinessUser {
            parts = parts.iter().map(business_line).collect();
        }
        let count = u32::try_from(parts.len()).unwrap_or(u32::MAX);
        shown = shown.saturating_add(count);
        groups.push(KindGroup {
            plain_kind_message_id: kind_info(first_kind).plain_message_id,
            count,
            parts,
        });
    }
    let pinned_by = match (&model.source, detail) {
        (
            crate::model::ModelSource::OrdinBundle {
                repository, commit, ..
            },
            Detail::LinksCode,
        ) => Some((repository.clone(), commit.clone())),
        _ => None,
    };
    InstalledMadeOf {
        display_name: model.display_name.clone(),
        what_it_does: model.description.clone(),
        tasks: model.tasks.iter().map(|task| task.name.clone()).collect(),
        part_count: shown,
        groups,
        pinned_by,
    }
}

/// Step 10: the folder's files, `ordin.lock` first, then by byte order of path.
pub(crate) fn installed_files(folder: &Path) -> Result<Vec<InstalledFileEntry>, InspectError> {
    let walked = walk(folder, u64::MAX)?;
    let mut entries = Vec::with_capacity(walked.len());
    for item in walked {
        let digest = match item.kind {
            EntryKind::File => {
                let bytes = std::fs::read(&item.path).map_err(|e| InspectError::folder(&e))?;
                sha256_hex(&bytes)
            }
            EntryKind::Link => {
                let target =
                    std::fs::read_link(&item.path).map_err(|e| InspectError::folder(&e))?;
                sha256_hex(target.to_string_lossy().as_bytes())
            }
        };
        entries.push(InstalledFileEntry {
            path: item.rel,
            size: item.size,
            sha256: digest,
        });
    }
    // `walk` sorted by byte order; the lock file moves to the front.
    if let Some(at) = entries
        .iter()
        .position(|entry| entry.path == ordin_agent_model::LOCK_FILE_NAME)
    {
        let lock = entries.remove(at);
        entries.insert(0, lock);
    }
    Ok(entries)
}

/// One file of the agent folder, for the reader of *What [agent] is made of*. The folder and the
/// file are both canonicalised (links resolved) and the file must lie inside the folder; anything
/// else, a folder or a missing file is `PartNotFound` with the path asked for, so a screen can never
/// read outside the agent's own directory.
pub(crate) fn installed_file(folder: &Path, path: &str) -> Result<InstalledFileText, InspectError> {
    let not_found = || InspectError::PartNotFound {
        part_id: path.to_owned(),
    };
    let root = folder
        .canonicalize()
        .map_err(|e| InspectError::folder(&e))?;
    let target = root.join(path).canonicalize().map_err(|_| not_found())?;
    if target == root || !target.starts_with(&root) || !target.is_file() {
        return Err(not_found());
    }
    let bytes = std::fs::read(&target).map_err(|e| InspectError::folder(&e))?;
    let mut text = strip_controls_keep_lines(&String::from_utf8_lossy(&bytes));
    let cut = text.len() > MAX_PAGE_BYTES;
    if cut {
        let mut end = MAX_PAGE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    Ok(InstalledFileText {
        path: path.to_owned(),
        text,
        cut,
    })
}

/// Step 11.
pub(crate) fn part_page(
    model: &PackageModel,
    part_id: &str,
    bundle_dir: &Path,
    detail: Detail,
) -> Result<PartPage, InspectError> {
    let line = model
        .parts
        .iter()
        .find(|line| line.part.id == part_id)
        .ok_or_else(|| InspectError::PartNotFound {
            part_id: part_id.to_owned(),
        })?;
    let hex = line
        .part
        .content_hash
        .strip_prefix("sha256:")
        .unwrap_or(&line.part.content_hash);
    let dir = bundle_dir.join("artefacts").join(hex);
    let (path, bytes, _) = primary_manifest(&dir).ok_or(InspectError::BundleUnreadable)?;
    let whole = String::from_utf8_lossy(&bytes).into_owned();
    let body = markdown_body(&path, &bytes).unwrap_or(whole);
    let mut text = strip_controls_keep_lines(&body);
    if text.len() > MAX_PAGE_BYTES {
        let mut end = MAX_PAGE_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n…");
    }
    Ok(PartPage {
        line: if detail == Detail::BusinessUser {
            business_line(line)
        } else {
            line.clone()
        },
        plain_kind_message_id: kind_info(line.part.kind).plain_message_id,
        text,
        file: (detail == Detail::LinksCode).then_some(path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh agent folder holding `skills/a.md`, with a secret beside it outside the folder.
    fn agent_folder() -> (std::path::PathBuf, std::path::PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "pki-installed-file-{}",
            ordin_types::AgentInstanceId::new()
        ));
        let folder = base.join("agent");
        std::fs::create_dir_all(folder.join("skills")).expect("dir");
        std::fs::write(folder.join("skills/a.md"), "# A\n\nDoes a.\u{7}").expect("file");
        std::fs::write(base.join("secret.txt"), "not the agent's").expect("file");
        (base, folder)
    }

    #[test]
    fn an_installed_file_is_read_as_text() {
        let (base, folder) = agent_folder();
        let file = installed_file(&folder, "skills/a.md").expect("read");
        assert_eq!(file.path, "skills/a.md");
        assert_eq!(file.text, "# A\n\nDoes a.");
        assert!(!file.cut);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_long_installed_file_is_cut() {
        let (base, folder) = agent_folder();
        std::fs::write(folder.join("big.txt"), "é".repeat(MAX_PAGE_BYTES)).expect("file");
        let file = installed_file(&folder, "big.txt").expect("read");
        assert!(file.cut);
        assert!(file.text.len() <= MAX_PAGE_BYTES);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_path_outside_the_agent_folder_is_refused() {
        let (base, folder) = agent_folder();
        let absolute = base.join("secret.txt");
        for path in [
            "../secret.txt",
            "skills/../../secret.txt",
            absolute.to_str().expect("utf-8"),
            "",
            ".",
            "skills",
            "missing.md",
        ] {
            assert!(
                matches!(
                    installed_file(&folder, path),
                    Err(InspectError::PartNotFound { .. })
                ),
                "{path:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_leading_out_of_the_agent_folder_is_refused() {
        let (base, folder) = agent_folder();
        std::os::unix::fs::symlink(base.join("secret.txt"), folder.join("link.md")).expect("link");
        assert!(matches!(
            installed_file(&folder, "link.md"),
            Err(InspectError::PartNotFound { .. })
        ));
        let _ = std::fs::remove_dir_all(&base);
    }
}
