//! Text normalisation shared by the write pipeline, the stores and the Service (memory-store
//! amendment January 2026, ADR-021).
//!
//! [`normalise_name`] is the one normaliser for entity names and aliases (SA-GRAPH-01): moved here from
//! the write pipeline's rules tier with its behaviour unchanged, so the name a write resolves, the
//! `names_norm` a store indexes and the phrase a recall matches are produced by the same function.

/// Normalise an entity name or alias (SA-GRAPH-01): NFC, lower-case, trim, collapse whitespace runs to
/// one space, strip leading and trailing ASCII punctuation.
///
/// NFC is the identity here, as it was in the write pipeline: input is expected composed (the
/// pipeline's documented assumption), and the crate takes no Unicode-tables dependency for it.
// spec: components/memory-store#SA-GRAPH-01
pub fn normalise_name(s: &str) -> String {
    let collapsed = collapse_ws(s).to_lowercase();
    collapsed
        .trim_matches(|c: char| c.is_ascii_punctuation())
        .trim()
        .to_string()
}

/// Normalise a link kind (SA-GRAPH-03): as [`normalise_name`] minus the punctuation stripping —
/// lower-case, trim, collapse whitespace.
// spec: components/memory-store#SA-GRAPH-03
pub fn normalise_kind(s: &str) -> String {
    collapse_ws(s).to_lowercase()
}

/// Normalise a triple's predicate (SA-TRIPLE-01): NFC, trim, collapse internal whitespace to one space.
/// Letter case is kept, because a caller's vocabulary names its terms case-sensitively (`worksFor`).
// spec: phase-1-analysis#SA-TRIPLE-01
pub fn normalise_predicate(s: &str) -> String {
    collapse_ws(s)
}

/// A predicate as words for keyword search and the embedding (SA-TRIPLE-03): `_`, `-` and every
/// boundary between a lower-case and an upper-case letter become one space (`worksFor` → `works For`).
// spec: phase-1-analysis#SA-TRIPLE-03
pub fn predicate_words(p: &str) -> String {
    let mut out = String::with_capacity(p.len() + 4);
    let mut prev: Option<char> = None;
    for c in p.chars() {
        if c == '_' || c == '-' {
            out.push(' ');
        } else {
            if let Some(pc) = prev {
                if pc.is_lowercase() && c.is_uppercase() {
                    out.push(' ');
                }
            }
            out.push(c);
        }
        prev = Some(c);
    }
    collapse_ws(&out)
}

/// Collapse internal whitespace runs to a single space and trim.
pub(crate) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sørensen–Dice coefficient over character bigrams of two strings (1.0 = identical). Strings too short
/// for a bigram compare by equality only.
pub(crate) fn dice_bigram(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let ba = bigrams(a);
    let bb = bigrams(b);
    if ba.is_empty() || bb.is_empty() {
        return 0.0;
    }
    let mut bb_counts: std::collections::HashMap<(char, char), usize> =
        std::collections::HashMap::new();
    for g in &bb {
        *bb_counts.entry(*g).or_default() += 1;
    }
    let mut overlap = 0usize;
    for g in &ba {
        if let Some(c) = bb_counts.get_mut(g) {
            if *c > 0 {
                *c -= 1;
                overlap += 1;
            }
        }
    }
    (2.0 * overlap as f64) / (ba.len() as f64 + bb.len() as f64)
}

fn bigrams(s: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = s.chars().collect();
    chars.windows(2).map(|w| (w[0], w[1])).collect()
}

/// The distinct normalised names of an entity: its canonical name, then each alias, empty results
/// skipped (step 7′, `names_norm`).
pub(crate) fn names_norm(canonical_name: &str, aliases: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(aliases.len() + 1);
    for n in std::iter::once(canonical_name).chain(aliases.iter().map(String::as_str)) {
        let n = normalise_name(n);
        if !n.is_empty() && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Maximum query phrases (SA-GRAPH-10).
const MAX_PHRASES: usize = 64;
/// Longest query phrase, in words (SA-GRAPH-10).
const MAX_PHRASE_WORDS: usize = 4;

/// The query phrases of SA-GRAPH-10: the 1- to 4-word n-grams of each normalised text, each n-gram
/// itself normalised (so the punctuation at its edges is stripped as a name's is), distinct, longest
/// first, at most 64.
// spec: components/memory-store#SA-GRAPH-10
pub(crate) fn query_phrases(texts: &[String]) -> Vec<String> {
    let words: Vec<Vec<String>> = texts
        .iter()
        .map(|t| {
            collapse_ws(t)
                .to_lowercase()
                .split(' ')
                .filter(|w| !w.is_empty())
                .map(str::to_string)
                .collect()
        })
        .collect();
    let mut out: Vec<String> = Vec::new();
    for n in (1..=MAX_PHRASE_WORDS).rev() {
        for ws in &words {
            if ws.len() < n {
                continue;
            }
            for window in ws.windows(n) {
                let p = normalise_name(&window.join(" "));
                if !p.is_empty() && !out.contains(&p) {
                    out.push(p);
                    if out.len() == MAX_PHRASES {
                        return out;
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_keep_their_case_and_lose_extra_whitespace() {
        assert_eq!(normalise_predicate("  works   For "), "works For");
        assert_eq!(normalise_predicate("accountManager"), "accountManager");
    }

    #[test]
    fn predicate_words_split_snake_kebab_and_camel_case() {
        let cases = [
            ("accountManager", "account Manager"),
            ("worksFor", "works For"),
            ("number_of_employees", "number of employees"),
            ("part-of", "part of"),
            ("URL", "URL"),
            ("owns", "owns"),
        ];
        for (p, want) in cases {
            assert_eq!(predicate_words(p), want, "{p}");
        }
    }

    #[test]
    fn names_are_lower_cased_trimmed_collapsed_and_stripped_of_edge_punctuation() {
        let cases = [
            ("  Borealis   Ltd. ", "borealis ltd"),
            ("\"Packaging\"", "packaging"),
            ("A-B", "a-b"),
            ("!!!", ""),
            ("Sam 07700", "sam 07700"),
        ];
        for (input, want) in cases {
            assert_eq!(normalise_name(input), want, "{input:?}");
        }
    }

    #[test]
    fn kinds_keep_their_punctuation() {
        assert_eq!(normalise_kind("  Supplies "), "supplies");
        assert_eq!(normalise_kind("Is-Part  Of."), "is-part of.");
    }

    #[test]
    fn names_norm_is_distinct_and_skips_empty_names() {
        let n = names_norm(
            "Borealis",
            &["borealis".into(), "Borealis Ltd".into(), "...".into()],
        );
        assert_eq!(n, vec!["borealis".to_string(), "borealis ltd".to_string()]);
    }

    #[test]
    fn query_phrases_are_longest_first_distinct_and_capped() {
        let p = query_phrases(&["Who supplies packaging?".to_string()]);
        assert_eq!(
            p,
            vec![
                "who supplies packaging",
                "who supplies",
                "supplies packaging",
                "who",
                "supplies",
                "packaging"
            ]
        );
        let long: String = (0..40).map(|i| format!("w{i} ")).collect();
        assert_eq!(query_phrases(&[long]).len(), 64);
    }

    #[test]
    fn dice_is_one_for_equal_and_zero_for_disjoint() {
        assert_eq!(dice_bigram("supplies", "supplies"), 1.0);
        assert_eq!(dice_bigram("ab", "cd"), 0.0);
        assert!(dice_bigram("supplies", "supplied") > 0.8);
    }
}
