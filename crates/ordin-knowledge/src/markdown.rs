//! Markdown reading without a Markdown library: code areas (§P step 1), page links (§P steps 2–3),
//! sections (§F step 2), the line diff (D-2), snippets and word tokens (H-8). Every scan is linear
//! in the body (bounded by the 1 MiB page bound), with no regular expression on page text.

use std::ops::Range;

// The Memory Service refuses a section text over 4,000 characters (MemoryService.md §8,
// `MEM_PAGE_SECTION_INVALID`), so a longer section is split into parts of at most 4,000
// characters under the same heading (KnowledgeService.md, SA-KBUILT-02).
pub(crate) const SECTION_TEXT_MAX: usize = 4_000;
pub(crate) const SECTIONS_MAX: usize = 200;

/// The lines of a text with their byte offsets (the line without its `\n`).
pub(crate) fn lines_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
        out.push((start, trimmed));
        start += line.len();
    }
    out
}

fn fence_of(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = rest.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Whether a line is a heading: up to 3 spaces, 1–6 `#`, then a space or the end.
pub(crate) fn heading_of(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let after = &rest[hashes..];
    if after.is_empty() {
        return Some("");
    }
    after.strip_prefix(' ').map(str::trim)
}

/// Byte ranges of fenced code blocks, indented code blocks and inline code spans.
pub(crate) fn code_ranges(body: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut previous_blank = true;
    let mut previous_indented = false;
    for (start, line) in lines_with_offsets(body) {
        let end = start + line.len();
        if let Some((c, n)) = fence {
            ranges.push(start..end);
            if let Some((c2, n2)) = fence_of(line) {
                if c2 == c && n2 >= n && line.trim().chars().all(|x| x == c) {
                    fence = None;
                }
            }
            previous_blank = false;
            continue;
        }
        if let Some(open) = fence_of(line) {
            fence = Some(open);
            ranges.push(start..end);
            previous_blank = false;
            previous_indented = false;
            continue;
        }
        let blank = line.trim().is_empty();
        let indented = !blank && (line.starts_with("    ") || line.starts_with('\t'));
        if indented && (previous_blank || previous_indented) {
            ranges.push(start..end);
            previous_indented = true;
            previous_blank = false;
            continue;
        }
        previous_indented = false;
        previous_blank = blank;
        inline_code(line, start, &mut ranges);
    }
    ranges
}

fn inline_code(line: &str, offset: usize, out: &mut Vec<Range<usize>>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run_start = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let n = i - run_start;
        // Look for a closing run of exactly n backticks on the same line.
        let mut j = i;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let s = j;
                while j < bytes.len() && bytes[j] == b'`' {
                    j += 1;
                }
                if j - s == n {
                    closed = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        if let Some(close) = closed {
            out.push(offset + run_start..offset + close);
            i = close;
        }
    }
}

pub(crate) fn in_ranges(ranges: &[Range<usize>], at: usize) -> bool {
    ranges.iter().any(|r| r.contains(&at))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkForm {
    Wiki,
    Embed,
    Markdown,
}

impl LinkForm {
    pub(crate) fn text(self) -> &'static str {
        match self {
            LinkForm::Wiki => "wiki",
            LinkForm::Embed => "embed",
            LinkForm::Markdown => "markdown",
        }
    }
}

/// One link as written in a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawLink {
    pub form: LinkForm,
    /// The whole link (`[[…]]`, `![[…]]`, `[…](…)`).
    pub span: Range<usize>,
    /// The target text inside it.
    pub target_span: Range<usize>,
    /// Wiki: the target with a trailing `.md` dropped. Markdown: the percent-decoded path,
    /// `.md` kept, before resolution against the linking file's folder.
    pub target: String,
    pub section: Option<String>,
    pub shown: Option<String>,
}

/// §P steps 1–3: every wiki and Markdown link outside code, in body order.
pub(crate) fn scan_links(body: &str) -> Vec<RawLink> {
    let code = code_ranges(body);
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if in_ranges(&code, i) {
            i += 1;
            continue;
        }
        if bytes[i] == b'[' && bytes.get(i + 1) == Some(&b'[') {
            if let Some(link) = wiki_at(body, i, &code) {
                i = link.span.end;
                out.push(link);
                continue;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'[' && (i == 0 || bytes[i - 1] != b'!') {
            if let Some(link) = markdown_at(body, i, &code) {
                i = link.span.end;
                out.push(link);
                continue;
            }
        }
        i += 1;
    }
    out
}

fn wiki_at(body: &str, open: usize, code: &[Range<usize>]) -> Option<RawLink> {
    let inner_start = open + 2;
    let rest = &body[inner_start..];
    let close_rel = rest.find("]]")?;
    let inner = &rest[..close_rel];
    if inner.contains('\n') || inner.contains("[[") {
        return None;
    }
    let close = inner_start + close_rel;
    if in_ranges(code, close) {
        return None;
    }
    let embed = open > 0 && body.as_bytes()[open - 1] == b'!';
    let start = if embed { open - 1 } else { open };
    let (target_part, shown) = match inner.split_once('|') {
        Some((t, s)) => (t, Some(s.trim().to_owned())),
        None => (inner, None),
    };
    let (target_raw, section) = match target_part.rsplit_once('#') {
        Some((t, s)) => {
            let s = s.trim();
            let section = (!s.starts_with('^') && !s.is_empty()).then(|| s.to_owned());
            (t, section)
        }
        None => (target_part, None),
    };
    let lead = target_raw.len() - target_raw.trim_start().len();
    let target_trimmed = target_raw.trim();
    if target_trimmed.is_empty() {
        return None;
    }
    let target = crate::names::strip_md(target_trimmed);
    if names_other_extension(target) {
        return None;
    }
    let target_start = inner_start + lead;
    Some(RawLink {
        form: if embed {
            LinkForm::Embed
        } else {
            LinkForm::Wiki
        },
        span: start..close + 2,
        target_span: target_start..target_start + target_trimmed.len(),
        target: target.to_owned(),
        section,
        shown: shown.filter(|s| !s.is_empty()),
    })
}

/// A target naming an extension other than `.md` (`image.png`) is no page link.
fn names_other_extension(target: &str) -> bool {
    let last = target.rsplit('/').next().unwrap_or(target);
    match last.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=8).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
        }
        None => false,
    }
}

fn markdown_at(body: &str, open: usize, code: &[Range<usize>]) -> Option<RawLink> {
    let rest = &body[open + 1..];
    let text_end_rel = rest.find(']')?;
    let text = &rest[..text_end_rel];
    if text.contains('\n') || text.contains('[') {
        return None;
    }
    let after = open + 1 + text_end_rel + 1;
    if body.as_bytes().get(after) != Some(&b'(') {
        return None;
    }
    let dest_rest = &body[after + 1..];
    let dest_end_rel = dest_rest.find(')')?;
    let dest_all = &dest_rest[..dest_end_rel];
    if dest_all.contains('\n') {
        return None;
    }
    let close = after + 1 + dest_end_rel;
    if in_ranges(code, close) {
        return None;
    }
    // A link title (`"…"`) after a space is not part of the destination.
    let dest = dest_all.split(" \"").next().unwrap_or(dest_all).trim();
    let dest = dest
        .strip_prefix('<')
        .and_then(|d| d.strip_suffix('>'))
        .unwrap_or(dest);
    let (path_raw, section_raw) = match dest.split_once('#') {
        Some((p, s)) => (p, Some(s)),
        None => (dest, None),
    };
    if path_raw.is_empty() || !path_raw.to_ascii_lowercase().ends_with(".md") {
        return None;
    }
    // No scheme: a `:` before the first `/`.
    let first_slash = path_raw.find('/').unwrap_or(path_raw.len());
    if path_raw[..first_slash].contains(':') {
        return None;
    }
    let path = percent_decode(path_raw)?;
    let section = section_raw
        .and_then(percent_decode)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && !s.starts_with('^'));
    let dest_offset = after + 1 + (dest_all.len() - dest_all.trim_start().len());
    Some(RawLink {
        form: LinkForm::Markdown,
        span: open..close + 1,
        target_span: dest_offset..dest_offset + path_raw.len(),
        target: path,
        section,
        shown: Some(text.to_owned()).filter(|t| !t.is_empty()),
    })
}

pub(crate) fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = raw.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Resolves a Markdown link path against the linking file's folder (below the bundle root), or
/// from the bundle root for a path beginning `/`; `None` when it leaves the bundle. The result is
/// the bundle-relative path without `.md`.
pub(crate) fn resolve_bundle_path(linking_folder: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let joined;
    let full = if let Some(abs) = path.strip_prefix('/') {
        abs
    } else {
        joined = if linking_folder.is_empty() {
            path.to_owned()
        } else {
            format!("{linking_folder}/{path}")
        };
        joined.as_str()
    };
    for seg in full.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join("/");
    Some(crate::names::strip_md(&joined).to_owned())
}

/// §F step 2: a line beginning with 1–6 `#` and a space starts a section; text before the first
/// heading forms a section headed with the page title; empty sections are dropped; a section's
/// text is split into parts of at most 4,000 characters; at most 200 sections. Headings inside
/// fenced code are text.
pub(crate) fn split_sections(title: &str, body: &str) -> Vec<(String, String)> {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut heading = title.to_owned();
    let mut text = String::new();
    let mut fence: Option<(char, usize)> = None;
    let push = |heading: &str, text: &str, sections: &mut Vec<(String, String)>| {
        let t = text.trim();
        if t.is_empty() {
            return;
        }
        // Parts on character boundaries, so a multi-byte character is never cut in two; a part of
        // white space only is dropped, as an empty section is.
        let chars: Vec<char> = t.chars().collect();
        for part in chars.chunks(SECTION_TEXT_MAX) {
            if sections.len() >= SECTIONS_MAX {
                return;
            }
            if part.iter().all(|c| c.is_whitespace()) {
                continue;
            }
            sections.push((heading.to_owned(), part.iter().collect()));
        }
    };
    for (_, line) in lines_with_offsets(body) {
        if let Some((c, n)) = fence {
            if let Some((c2, n2)) = fence_of(line) {
                if c2 == c && n2 >= n {
                    fence = None;
                }
            }
            text.push_str(line);
            text.push('\n');
            continue;
        }
        if let Some(open) = fence_of(line) {
            fence = Some(open);
            text.push_str(line);
            text.push('\n');
            continue;
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        let rest = &line[indent.min(3).min(line.len())..];
        let hashes = rest.chars().take_while(|c| *c == '#').count();
        if indent == 0 && (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
            push(&heading, &text, &mut sections);
            heading = rest[hashes..].trim().to_owned();
            text.clear();
            continue;
        }
        text.push_str(line);
        text.push('\n');
    }
    push(&heading, &text, &mut sections);
    sections
}

/// The number of lines inserted or deleted by a line diff (D-2): N + M − 2·LCS, computed with
/// Myers' greedy algorithm in O((N+M)·D) time and O(N+M) memory.
pub(crate) fn changed_lines(before: &str, after: &str) -> u32 {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let n = a.len() as isize;
    let m = b.len() as isize;
    let max = (n + m) as usize;
    if max == 0 {
        return 0;
    }
    let offset = max as isize;
    let mut v = vec![0isize; 2 * max + 2];
    for d in 0..=max as isize {
        let mut k = -d;
        while k <= d {
            let idx = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x >= n && y >= m {
                return u32::try_from(d).unwrap_or(u32::MAX);
            }
            k += 2;
        }
    }
    u32::try_from(max).unwrap_or(u32::MAX)
}

/// H-8 tokens: runs of letters and digits, lower-cased.
pub(crate) fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The FTS5 any-word query: each token double-quoted (an embedded `"` doubled) and joined with
/// `OR`; `None` when the query holds no token, so query syntax in the input is inert.
pub(crate) fn fts_any_word(query: &str) -> Option<String> {
    let toks = tokens(query);
    if toks.is_empty() {
        return None;
    }
    Some(
        toks.iter()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// The FTS5 phrase query of a name (U-1).
pub(crate) fn fts_phrase(name: &str) -> Option<String> {
    let toks = tokens(name);
    if toks.is_empty() {
        return None;
    }
    Some(format!("\"{}\"", toks.join(" ").replace('"', "\"\"")))
}

/// Plain text of a body: no Markdown markers, one space between lines.
pub(crate) fn plain_text(body: &str) -> String {
    let mut out = String::with_capacity(body.len().min(4096));
    for (_, line) in lines_with_offsets(body) {
        let l = line.trim_start_matches(['#', '>', ' ']);
        for c in l.chars() {
            if !matches!(c, '*' | '_' | '`' | '[' | ']' | '|') {
                out.push(c);
            }
        }
        out.push(' ');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A 300-character plain-text snippet around the first query word found, else the start.
// FTS5 `snippet()` returns nothing on the contentless `knowledge_pages_fts` (`content=''`), so the
// snippet is cut from the body (KnowledgeService.md, SA-KBUILT-03).
pub(crate) fn snippet(body: &str, query: &str, chars: usize) -> String {
    let plain = plain_text(body);
    let lower = plain.to_lowercase();
    let at = tokens(query)
        .iter()
        .filter_map(|t| lower.find(t.as_str()))
        .min()
        .unwrap_or(0);
    // Back up to a character position roughly a quarter of the window before the match.
    let char_at = plain[..at.min(plain.len())].chars().count();
    let start = char_at.saturating_sub(chars / 4);
    plain.chars().skip(start).take(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_link_form_is_read() {
        let body = "a [[Supplier rates]] b [[rate rules|the rules]] c [[Supplier rates#Review cycle]] d [x](Supplier%20rates.md) e ![[Pic.png]] f [[#Local]] g ![[Embed me]]";
        let links = scan_links(body);
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(
            targets,
            [
                "Supplier rates",
                "rate rules",
                "Supplier rates",
                "Supplier rates.md",
                "Embed me"
            ]
        );
        assert_eq!(links[2].section.as_deref(), Some("Review cycle"));
        assert_eq!(links[1].shown.as_deref(), Some("the rules"));
        assert_eq!(links[4].form, LinkForm::Embed);
        assert_eq!(&body[links[0].target_span.clone()], "Supplier rates");
    }

    #[test]
    fn links_in_code_are_not_links() {
        let body = "```\n[[Supplier rates]]\n```\nand `[[x]]` and\n\n    [[indented]]\n";
        assert!(scan_links(body).is_empty());
    }

    #[test]
    fn markdown_links_need_md_and_no_scheme() {
        assert!(scan_links("[a](https://x.example/a.md)").is_empty());
        assert!(scan_links("[a](b.txt)").is_empty());
        assert_eq!(
            scan_links("[a](/dir/b.md#Top)")[0].section.as_deref(),
            Some("Top")
        );
        assert_eq!(resolve_bundle_path("x", "../y.md").as_deref(), Some("y"));
        assert_eq!(resolve_bundle_path("", "../y.md"), None);
        assert_eq!(resolve_bundle_path("x", "/z/y.md").as_deref(), Some("z/y"));
    }

    #[test]
    fn sections_follow_headings() {
        let s = split_sections(
            "T",
            "intro\n## Review\nRates.\n\n## Empty\n\n## Exceptions\nK.",
        );
        assert_eq!(
            s,
            vec![
                ("T".into(), "intro".into()),
                ("Review".into(), "Rates.".into()),
                ("Exceptions".into(), "K.".into())
            ]
        );
    }

    #[test]
    fn a_section_over_the_memory_service_limit_is_split_on_character_boundaries() {
        // 9,000 two-byte characters: three parts of 4,000, 4,000 and 1,000 under one heading.
        let long = "é".repeat(9_000);
        let s = split_sections("T", &format!("## Rates\n{long}\n## Next\nK."));
        let counts: Vec<(&str, usize)> = s
            .iter()
            .map(|(h, t)| (h.as_str(), t.chars().count()))
            .collect();
        assert_eq!(
            counts,
            [
                ("Rates", 4_000),
                ("Rates", 4_000),
                ("Rates", 1_000),
                ("Next", 2)
            ]
        );
        assert_eq!(
            s[..3].iter().map(|(_, t)| t.as_str()).collect::<String>(),
            long
        );
        // A part of white space only is dropped, and the 200-section bound still holds.
        let spaced = format!("a{}b", " ".repeat(8_000));
        let s = split_sections("T", &spaced);
        assert!(s.iter().all(|(_, t)| !t.trim().is_empty()));
        let many = "x".repeat(SECTION_TEXT_MAX * 250);
        assert_eq!(split_sections("T", &many).len(), SECTIONS_MAX);
    }

    #[test]
    fn line_diff_counts_insertions_and_deletions() {
        assert_eq!(changed_lines("a\nb\nc", "a\nb\nc"), 0);
        assert_eq!(changed_lines("a\nb\nc", "a\nx\nc"), 2);
        assert_eq!(changed_lines("", "a\nb"), 2);
    }

    #[test]
    fn fts_query_quotes_tokens() {
        assert_eq!(
            fts_any_word("rotate \"deploy\" OR key*").as_deref(),
            Some("\"rotate\" OR \"deploy\" OR \"or\" OR \"key\"")
        );
        assert_eq!(fts_any_word("  ** "), None);
    }
}
