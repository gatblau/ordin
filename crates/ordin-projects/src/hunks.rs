//! In-crate unified-hunk application (Internal Logic 11.2) and the reading of merge conflict
//! markers. `git2` applies whole patches to a tree or index only; review works on one hunk of one
//! file's bytes.

/// One side of a split hunk header `@@ -a,b +c,d @@`.
fn range(text: &str) -> Option<(usize, usize)> {
    let (start, count) = match text.split_once(',') {
        Some((start, count)) => (start, count.parse().ok()?),
        None => (text, 1),
    };
    Some((start.parse().ok()?, count))
}

/// `(old start, old count)` of a hunk's header line.
fn header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, _) = rest.split_once(' ')?;
    range(old)
}

/// `bytes` split after each line feed; the last line may have none.
fn lines_of(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|b| *b == b'\n').collect()
}

fn without_newline(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").unwrap_or(line)
}

/// `ancestor` with this one hunk applied; `None` when the hunk does not apply to it.
pub(crate) fn apply_hunk(ancestor: &[u8], hunk: &str) -> Option<Vec<u8>> {
    let mut hunk_lines = hunk.split_inclusive('\n');
    let (old_start, old_count) = header(hunk_lines.next()?.trim_end())?;
    let old = lines_of(ancestor);
    // A count of zero names the line after which the hunk goes.
    let at = if old_count == 0 {
        old_start
    } else {
        old_start.checked_sub(1)?
    };
    if at > old.len() {
        return None;
    }
    let mut out: Vec<u8> = old[..at].concat();
    let mut position = at;
    let mut body = hunk_lines.peekable();
    while let Some(line) = body.next() {
        let no_newline = body.peek().is_some_and(|next| next.starts_with('\\'));
        if line.starts_with('\\') {
            continue;
        }
        let (mark, text) = line.split_at(1);
        let content = text.strip_suffix('\n').unwrap_or(text).as_bytes();
        match mark {
            " " | "-" => {
                let original = old.get(position)?;
                if without_newline(original) != content {
                    return None;
                }
                position += 1;
                if mark == " " {
                    out.extend_from_slice(original);
                }
            }
            "+" => {
                out.extend_from_slice(content);
                if !no_newline {
                    out.push(b'\n');
                }
            }
            _ => return None,
        }
    }
    out.extend_from_slice(&old[position..].concat());
    Some(out)
}

/// The conflicting regions of a merge result written with standard markers: each side's regions,
/// joined.
pub(crate) fn conflict_regions(merged: &[u8]) -> (String, String) {
    enum Side {
        Outside,
        Ours,
        Theirs,
    }
    let (mut yours, mut agents) = (Vec::new(), Vec::new());
    let mut side = Side::Outside;
    for line in lines_of(merged) {
        match side {
            Side::Outside if line.starts_with(b"<<<<<<< ") => side = Side::Ours,
            Side::Ours if without_newline(line) == b"=======" => side = Side::Theirs,
            Side::Theirs if line.starts_with(b">>>>>>> ") => side = Side::Outside,
            Side::Ours => yours.extend_from_slice(line),
            Side::Theirs => agents.extend_from_slice(line),
            Side::Outside => {}
        }
    }
    (
        String::from_utf8_lossy(&yours).into_owned(),
        String::from_utf8_lossy(&agents).into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hunk_changes_the_lines_it_names() {
        let hunk = "@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
        assert_eq!(apply_hunk(b"a\nb\nc\n", hunk).unwrap(), b"a\nB\nc\n");
    }

    #[test]
    fn a_new_file_applies_to_nothing() {
        assert_eq!(
            apply_hunk(b"", "@@ -0,0 +1,2 @@\n+a\n+b\n").unwrap(),
            b"a\nb\n"
        );
    }

    #[test]
    fn an_insertion_goes_after_the_line_a_count_of_zero_names() {
        assert_eq!(
            apply_hunk(b"a\nb\n", "@@ -1,0 +2 @@\n+x\n").unwrap(),
            b"a\nx\nb\n"
        );
    }

    #[test]
    fn a_missing_final_newline_is_kept_either_way() {
        let added =
            "@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+b\n\\ No newline at end of file\n";
        assert_eq!(apply_hunk(b"a", added).unwrap(), b"b");
        let restored = "@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+a\n";
        assert_eq!(apply_hunk(b"a", restored).unwrap(), b"a\n");
    }

    #[test]
    fn a_hunk_that_does_not_match_does_not_apply() {
        assert!(apply_hunk(b"x\n", "@@ -1 +1 @@\n-a\n+b\n").is_none());
        assert!(apply_hunk(b"a\n", "@@ -9 +9 @@\n-a\n+b\n").is_none());
        assert!(apply_hunk(b"a\n", "not a hunk").is_none());
        assert!(apply_hunk(b"a\n", "@@ -1 +1 @@\n?a\n").is_none());
    }

    #[test]
    fn conflict_regions_are_read_side_by_side() {
        let merged = b"top\n<<<<<<< ours\nmine\n=======\nagent\n>>>>>>> theirs\nend\n";
        assert_eq!(
            conflict_regions(merged),
            ("mine\n".to_owned(), "agent\n".to_owned())
        );
    }
}
