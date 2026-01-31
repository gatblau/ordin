//! Step 6b: JSON Pointers of a binding's `argumentPaths` converted to the Rules Engine's place form.

/// One pointer as a place, or None when it cannot be expressed (the caller logs it).
///
/// Drop the leading `/`; split on `/`; unescape each token (`~1` to `/`, `~0` to `~`); join the
/// tokens with `.`, except that a token `[]` is appended to the previous token without a `.` and a
/// final token `*` becomes the suffix `.*`. A token that, once unescaped, contains `.`, `[` or `]`
/// (other than the whole token `[]`) cannot be expressed, nor can an empty token or a list marker
/// or wildcard with no token before it.
// spec: ordin-desktop/components/PackageInspector#SA-CLOSE-02
pub(crate) fn place_of(pointer: &str) -> Option<String> {
    let rest = pointer.strip_prefix('/')?;
    let tokens: Vec<String> = rest
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect();
    let last = tokens.len().checked_sub(1)?;
    let mut place = String::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.is_empty() {
            return None;
        }
        if token == "[]" {
            if place.is_empty() {
                return None;
            }
            place.push_str("[]");
            continue;
        }
        if index == last && token == "*" {
            if place.is_empty() {
                return None;
            }
            place.push_str(".*");
            continue;
        }
        if token.contains(['.', '[', ']']) {
            return None;
        }
        if !place.is_empty() {
            place.push('.');
        }
        place.push_str(token);
    }
    Some(place)
}

#[cfg(test)]
mod tests {
    use super::place_of;

    #[test]
    fn list_items_and_wildcards() {
        assert_eq!(place_of("/to/[]/address").as_deref(), Some("to[].address"));
        assert_eq!(place_of("/payload/*").as_deref(), Some("payload.*"));
        assert_eq!(place_of("/url").as_deref(), Some("url"));
        assert_eq!(place_of("/a/b/c").as_deref(), Some("a.b.c"));
    }

    #[test]
    fn escapes_are_undone_before_the_check() {
        assert_eq!(place_of("/a~0b").as_deref(), Some("a~b"));
        assert_eq!(place_of("/a~1b").as_deref(), Some("a/b"));
    }

    #[test]
    fn places_that_cannot_be_expressed_are_left_out() {
        assert_eq!(place_of("/a.b"), None);
        assert_eq!(place_of("/a[0]"), None);
        assert_eq!(place_of("/a]"), None);
        assert_eq!(place_of("/[]/a"), None);
        assert_eq!(place_of("/*"), None);
        assert_eq!(place_of("/a//b"), None);
        assert_eq!(place_of(""), None);
        assert_eq!(place_of("no-slash"), None);
    }
}
