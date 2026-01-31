//! `@`-import inlining for `instructions.md` (Installers.md *Internal Logic* step 3, C-61 (b)).
//!
//! Claude Code under `--restricted` reads no `CLAUDE.md`, so the text it would have imported is
//! inlined into the file passed with `--append-system-prompt-file`.

use crate::error::InstallError;
use crate::types::MAX_IMPORT_DEPTH;

/// Where an imported file is looked up: the rendered map, or the files under a release bundle.
pub(crate) trait ImportSource {
    /// The bytes of the '/'-separated relative path, or None when there is no such file.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// Inlines every `@<path>` line of `text`, read as the file at `path` (relative to the tree's root).
/// `missing` is called with each import target that names no file; the line is then left unchanged.
/// Output has LF line ends and exactly one trailing newline.
// spec: ordin-desktop/components/Installers#C-61
pub(crate) fn inline(
    text: &str,
    path: &str,
    source: &dyn ImportSource,
    missing: &mut dyn FnMut(&str),
) -> Result<String, InstallError> {
    let mut stack = vec![path.to_owned()];
    let expanded = expand(text, path, source, &mut stack, missing)?;
    let mut out = expanded.trim_end_matches('\n').to_owned();
    out.push('\n');
    Ok(out)
}

fn expand(
    text: &str,
    path: &str,
    source: &dyn ImportSource,
    stack: &mut Vec<String>,
    missing: &mut dyn FnMut(&str),
) -> Result<String, InstallError> {
    let text = text.replace("\r\n", "\n");
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.split('\n') {
        let trimmed = line.trim();
        // Lines inside a fenced code block (``` or ~~~) are never imports.
        if let Some(marker) = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m)) {
            fence = match fence {
                None => Some(marker),
                Some(open) if open == marker => None,
                Some(open) => Some(open),
            };
            out.push(line.to_owned());
            continue;
        }
        if fence.is_some() {
            out.push(line.to_owned());
            continue;
        }
        let Some(target) = import_target(trimmed) else {
            out.push(line.to_owned());
            continue;
        };
        let Some((resolved, bytes)) = find(path, target, source) else {
            missing(target);
            out.push(line.to_owned());
            continue;
        };
        // The importing chain holds CLAUDE.md at depth 0; an import met again on the current path
        // is a cycle, and one more than `MAX_IMPORT_DEPTH` deep is refused the same way.
        if stack.contains(&resolved) || stack.len() > MAX_IMPORT_DEPTH {
            return Err(InstallError::ImportCycle { file: resolved });
        }
        let imported = strip_front_matter(&String::from_utf8_lossy(&bytes).replace("\r\n", "\n"));
        stack.push(resolved.clone());
        let inner = expand(&imported, &resolved, source, stack, missing)?;
        stack.pop();
        out.push(inner.trim_end_matches('\n').to_owned());
    }
    Ok(out.join("\n"))
}

/// `^@([^\s]+)$` on the trimmed line.
fn import_target(trimmed: &str) -> Option<&str> {
    let target = trimmed.strip_prefix('@')?;
    (!target.is_empty() && !target.chars().any(char::is_whitespace)).then_some(target)
}

/// The importing file's directory first; then the tree's root.
// The scenario *Error — circular import* has `.claude/rules/a.md` import itself as
// `@.claude/rules/a.md`, a path that only resolves from the root, so both are tried.
fn find(importer: &str, target: &str, source: &dyn ImportSource) -> Option<(String, Vec<u8>)> {
    let dir = importer.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let relative = if dir.is_empty() {
        normalise(target)
    } else {
        normalise(&format!("{dir}/{target}"))
    };
    [relative, normalise(target)]
        .into_iter()
        .flatten()
        .find_map(|candidate| source.read(&candidate).map(|bytes| (candidate, bytes)))
}

/// Resolves `.` and `..` segments; None for an absolute path, a home path, a backslash, a NUL or a
/// path that leaves the tree.
fn normalise(path: &str) -> Option<String> {
    if path.starts_with('/') || path.starts_with('~') || path.contains('\\') || path.contains('\0')
    {
        return None;
    }
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// Removes a leading `---` … `---` block.
fn strip_front_matter(text: &str) -> String {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text.to_owned();
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches('\n') == "---" {
            return rest[offset..].to_owned();
        }
    }
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct Map(BTreeMap<String, Vec<u8>>);

    impl ImportSource for Map {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    fn map(files: &[(&str, &str)]) -> Map {
        Map(files
            .iter()
            .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
            .collect())
    }

    fn run(text: &str, files: &[(&str, &str)]) -> (Result<String, InstallError>, Vec<String>) {
        let mut seen = Vec::new();
        let out = inline(text, "CLAUDE.md", &map(files), &mut |t| {
            seen.push(t.to_owned())
        });
        (out, seen)
    }

    #[test]
    fn imports_are_inlined_with_front_matter_removed() {
        let (out, missing) = run(
            "# Head\r\n\n@.claude/rules/a.md\n@nowhere.md\n",
            &[
                (
                    ".claude/rules/a.md",
                    "---\nkind: rule\n---\nRule A.\n@b.md\n",
                ),
                (".claude/rules/b.md", "Rule B.\n"),
            ],
        );
        assert_eq!(out.unwrap(), "# Head\n\nRule A.\nRule B.\n@nowhere.md\n");
        assert_eq!(missing, vec!["nowhere.md".to_owned()]);
    }

    #[test]
    fn fenced_lines_are_left_alone() {
        let (out, missing) = run(
            "```\n@a.md\n```\n~~~\n```\n@a.md\n~~~\n@a.md",
            &[("a.md", "A")],
        );
        assert_eq!(out.unwrap(), "```\n@a.md\n```\n~~~\n```\n@a.md\n~~~\nA\n");
        assert!(missing.is_empty());
    }

    #[test]
    fn depth_above_five_is_refused() {
        let files = [
            ("d1.md", "@d2.md"),
            ("d2.md", "@d3.md"),
            ("d3.md", "@d4.md"),
            ("d4.md", "@d5.md"),
            ("d5.md", "five"),
        ];
        assert_eq!(run("@d1.md", &files).0.unwrap(), "five\n");
        let deeper = [
            ("d1.md", "@d2.md"),
            ("d2.md", "@d3.md"),
            ("d3.md", "@d4.md"),
            ("d4.md", "@d5.md"),
            ("d5.md", "@d6.md"),
            ("d6.md", "six"),
        ];
        assert_eq!(
            run("@d1.md", &deeper).0.unwrap_err(),
            InstallError::ImportCycle {
                file: "d6.md".to_owned()
            }
        );
    }

    #[test]
    fn paths_outside_the_tree_are_missing() {
        for target in ["/etc/passwd", "~/x.md", "../x.md", "a\\b.md", "./"] {
            let (out, missing) = run(&format!("@{target}"), &[("x.md", "X")]);
            assert_eq!(out.unwrap(), format!("@{target}\n"));
            assert_eq!(missing, vec![target.to_owned()]);
        }
        assert_eq!(normalise("a/./b/../c"), Some("a/c".to_owned()));
    }

    #[test]
    fn a_line_with_more_than_the_target_is_not_an_import() {
        let (out, _) = run("@a.md and more\n@\n", &[("a.md", "A")]);
        assert_eq!(out.unwrap(), "@a.md and more\n@\n");
        assert_eq!(strip_front_matter("---\nunclosed"), "---\nunclosed");
    }
}
