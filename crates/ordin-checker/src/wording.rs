//! Step 14: the wording rules (ADR-085) — "ordin tidied N things". Pure: named text fields in, the
//! tidied text, the changes and the findings out.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use ordin_types::domain::agents::WordingChange;
use regex::Regex;

use crate::finding::{finding, msg};
use crate::model::{Finding, Fix, Severity};

/// The British spellings of the `-ize` family: every stem takes each suffix.
const STEMS: [&str; 20] = [
    "organiz",
    "analyz",
    "recogniz",
    "prioritiz",
    "summariz",
    "optimiz",
    "minimiz",
    "maximiz",
    "customiz",
    "authoriz",
    "finaliz",
    "utiliz",
    "standardiz",
    "realiz",
    "categoriz",
    "characteriz",
    "emphasiz",
    "normaliz",
    "synchroniz",
    "visualiz",
];
const SUFFIXES: [&str; 8] = ["e", "es", "ed", "ing", "ation", "ations", "er", "ers"];
/// Whole words; `license` is left alone (its noun position cannot be detected).
const WORDS: [(&str, &str); 18] = [
    ("color", "colour"),
    ("behavior", "behaviour"),
    ("favorite", "favourite"),
    ("honor", "honour"),
    ("labor", "labour"),
    ("center", "centre"),
    ("catalog", "catalogue"),
    ("defense", "defence"),
    ("gray", "grey"),
    ("modeling", "modelling"),
    ("modeled", "modelled"),
    ("canceled", "cancelled"),
    ("canceling", "cancelling"),
    ("traveling", "travelling"),
    ("labeled", "labelled"),
    ("labeling", "labelling"),
    ("fulfill", "fulfil"),
    ("enrollment", "enrolment"),
];

/// W5: the imperative verbs a task name may start with (*list chosen here*).
const VERBS: &[&str] = &[
    "add",
    "analyse",
    "answer",
    "apply",
    "approve",
    "archive",
    "ask",
    "assess",
    "build",
    "calculate",
    "change",
    "check",
    "choose",
    "clean",
    "collect",
    "compare",
    "compile",
    "confirm",
    "count",
    "create",
    "decide",
    "define",
    "delete",
    "describe",
    "design",
    "draft",
    "estimate",
    "explain",
    "export",
    "extract",
    "file",
    "find",
    "fix",
    "flag",
    "format",
    "gather",
    "generate",
    "group",
    "import",
    "improve",
    "inspect",
    "list",
    "map",
    "mark",
    "match",
    "measure",
    "merge",
    "monitor",
    "move",
    "note",
    "open",
    "order",
    "plan",
    "prepare",
    "price",
    "publish",
    "rank",
    "rate",
    "read",
    "record",
    "reconcile",
    "refresh",
    "remove",
    "rename",
    "report",
    "request",
    "review",
    "revise",
    "rewrite",
    "run",
    "schedule",
    "score",
    "search",
    "send",
    "sort",
    "split",
    "summarise",
    "tag",
    "test",
    "track",
    "translate",
    "update",
    "validate",
    "verify",
    "write",
];

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// LD-21 limits.
const MAX_NAME: usize = 80;
const MAX_PURPOSE: usize = 500;
const MAX_ITEM: usize = 500;
const MAX_ITEMS: usize = 50;

fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// Back-tick code spans, URLs and ordin identifiers: never changed.
static PROTECTED: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"`[^`]*`|https?://\S+|urn:ordin:\S+"));
static WORD: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"[A-Za-z]+"));
static ISO_DATE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"\b(\d{4})-(\d{2})-(\d{2})\b"));
static SLASH_DATE: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"\b\d{1,2}/\d{1,2}/\d{2,4}\b"));
static DIGITS: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"\d{5,}"));
static BLANKS: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"[ \t]{2,}|\t"));

static SPELLINGS: LazyLock<BTreeMap<String, String>> = LazyLock::new(|| {
    let mut map = BTreeMap::new();
    for stem in STEMS {
        let british = format!("{}s", &stem[..stem.len() - 1]);
        for suffix in SUFFIXES {
            map.insert(format!("{stem}{suffix}"), format!("{british}{suffix}"));
        }
    }
    for (american, british) in WORDS {
        map.insert(american.to_owned(), british.to_owned());
    }
    map
});

/// Applies `f` to the prose between protected spans.
fn on_prose(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let Some(protected) = PROTECTED.as_ref() else {
        return f(text);
    };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for span in protected.find_iter(text) {
        out.push_str(&f(&text[at..span.start()]));
        out.push_str(span.as_str());
        at = span.end();
    }
    out.push_str(&f(&text[at..]));
    out
}

/// W1: trimmed, runs of spaces and tabs as one space.
fn whitespace(text: &str) -> String {
    let trimmed = text.trim();
    match BLANKS.as_ref() {
        Some(re) => re.replace_all(trimmed, " ").into_owned(),
        None => trimmed.to_owned(),
    }
}

/// The replacement's case follows the word's: all capitals, a capital first letter, or lower case.
fn with_case_of(word: &str, replacement: &str) -> String {
    let mut chars = word.chars();
    let first_upper = chars.next().is_some_and(char::is_uppercase);
    if first_upper && word.len() > 1 && word.chars().all(char::is_uppercase) {
        replacement.to_uppercase()
    } else if first_upper {
        capitalise(replacement)
    } else {
        replacement.to_owned()
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// W2: whole-word, case-preserving British spelling.
fn british(prose: &str) -> String {
    let Some(word) = WORD.as_ref() else {
        return prose.to_owned();
    };
    word.replace_all(prose, |caps: &regex::Captures<'_>| {
        let found = &caps[0];
        match SPELLINGS.get(&found.to_lowercase()) {
            Some(replacement) => with_case_of(found, replacement),
            None => found.to_owned(),
        }
    })
    .into_owned()
}

fn is_leap(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

fn valid_date(year: u32, month: u32, day: u32) -> bool {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// W3: an ISO date written out; a slash date is ambiguous.
fn dates(prose: &str, ambiguous: &mut bool) -> String {
    if SLASH_DATE.as_ref().is_some_and(|re| re.is_match(prose)) {
        *ambiguous = true;
    }
    let Some(iso) = ISO_DATE.as_ref() else {
        return prose.to_owned();
    };
    iso.replace_all(prose, |caps: &regex::Captures<'_>| {
        let year: u32 = caps[1].parse().unwrap_or(0);
        let month: u32 = caps[2].parse().unwrap_or(0);
        let day: u32 = caps[3].parse().unwrap_or(0);
        if valid_date(year, month, day) {
            let name = MONTHS[usize::try_from(month - 1).unwrap_or(0)];
            format!("{day} {name} {year}")
        } else {
            caps[0].to_owned()
        }
    })
    .into_owned()
}

/// W4: an integer of five or more digits standing alone in prose takes comma thousands groups.
fn numbers(prose: &str) -> String {
    let Some(digits) = DIGITS.as_ref() else {
        return prose.to_owned();
    };
    let mut out = String::with_capacity(prose.len());
    let mut at = 0;
    for m in digits.find_iter(prose) {
        let before = prose[..m.start()].chars().next_back();
        let mut after = prose[m.end()..].chars();
        let next = after.next();
        let standalone_before = before.is_none_or(|c| c.is_whitespace() || c == '(');
        let standalone_after = match next {
            None => true,
            Some(c) if c.is_whitespace() || matches!(c, ')' | ',' | ';' | '!' | '?') => true,
            Some('.') => after.next().is_none_or(char::is_whitespace),
            Some(_) => false,
        };
        out.push_str(&prose[at..m.start()]);
        if standalone_before && standalone_after {
            out.push_str(&group(m.as_str()));
        } else {
            out.push_str(m.as_str());
        }
        at = m.end();
    }
    out.push_str(&prose[at..]);
    out
}

fn group(digits: &str) -> String {
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// W5: the task name as it starts with a listed verb, or None when it cannot.
fn verb_first(task: &str) -> Option<String> {
    let (first, rest) = match task.find(char::is_whitespace) {
        Some(at) => (&task[..at], &task[at..]),
        None => (task, ""),
    };
    let lower = first.to_lowercase();
    if VERBS.contains(&lower.as_str()) {
        return Some(format!("{}{rest}", capitalise(first)));
    }
    let stem = lower.strip_suffix("ing")?.to_owned();
    let with_e = format!("{stem}e");
    let verb = [stem, with_e]
        .into_iter()
        .find(|candidate| VERBS.contains(&candidate.as_str()))?;
    Some(format!("{}{rest}", capitalise(&verb)))
}

/// The list a field belongs to (`tasks[2]` → `tasks`), None for a single field.
fn list_of(field: &str) -> Option<&str> {
    field.split_once('[').map(|(list, _)| list)
}

fn too_long(field: &str, detail: String) -> Finding {
    finding(
        "text_too_long",
        Severity::Failure,
        None,
        msg::TEXT_TOO_LONG,
        &[("field", field)],
        vec![Fix::ChangeItYourself {
            field: field.to_owned(),
        }],
        detail,
    )
}

/// Step 14 over named fields. A field named `tasks[i]` is a task name (W5); `name` and `purpose`
/// and each list item carry the LD-21 limits (W6). Languages other than en-GB get W1 and W6 only.
// spec: ordin-desktop/components/Checker#ADR-085
pub(crate) fn apply(
    fields: &[(String, String)],
    language: &str,
) -> (Vec<(String, String)>, Vec<WordingChange>, Vec<Finding>) {
    let english = language.starts_with("en-GB");
    let mut tidied = Vec::with_capacity(fields.len());
    let mut changes = Vec::new();
    let mut findings = Vec::new();
    let mut list_sizes: BTreeMap<&str, usize> = BTreeMap::new();
    for (field, original) in fields {
        let mut text = whitespace(original);
        if english {
            let mut ambiguous = false;
            text = on_prose(&text, |prose| {
                numbers(&dates(&british(prose), &mut ambiguous))
            });
            if ambiguous {
                findings.push(finding(
                    "date_ambiguous",
                    Severity::Failure,
                    None,
                    msg::DATE_AMBIGUOUS,
                    &[("field", field)],
                    vec![Fix::ChangeItYourself {
                        field: field.clone(),
                    }],
                    format!("{field} holds a date whose day and month order cannot be known"),
                ));
            }
            if list_of(field) == Some("tasks") && !text.is_empty() {
                match verb_first(&text) {
                    Some(verb) => text = verb,
                    None => findings.push(finding(
                        "task_name_not_verb",
                        Severity::Failure,
                        None,
                        msg::TASK_NAME_NOT_VERB,
                        &[("task", &text)],
                        vec![
                            Fix::AskOrdin,
                            Fix::ChangeItYourself {
                                field: "tasks".to_owned(),
                            },
                        ],
                        format!("{field} does not start with a verb"),
                    )),
                }
            }
        }
        let limit = match (field.as_str(), list_of(field)) {
            ("name", _) => Some(MAX_NAME),
            ("purpose", _) => Some(MAX_PURPOSE),
            (_, Some(_)) => Some(MAX_ITEM),
            _ => None,
        };
        let chars = text.chars().count();
        if let Some(limit) = limit.filter(|l| chars > *l) {
            findings.push(too_long(
                field,
                format!("{field} has {chars} characters; at most {limit}"),
            ));
        }
        if let Some(list) = list_of(field) {
            *list_sizes.entry(list).or_default() += 1;
        }
        if text != *original {
            changes.push(WordingChange {
                field: field.clone(),
                before: original.clone(),
                after: text.clone(),
            });
        }
        tidied.push((field.clone(), text));
    }
    for (list, size) in list_sizes {
        if size > MAX_ITEMS {
            findings.push(too_long(
                list,
                format!("{list} has {size} items; at most {MAX_ITEMS}"),
            ));
        }
    }
    (tidied, changes, findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelling_keeps_case_and_skips_protected_spans() {
        let fields = vec![(
            "purpose".to_owned(),
            "Analyzes COLOR and `analyze` at https://x.example/analyze for urn:ordin:a:skills:organize"
                .to_owned(),
        )];
        let (tidied, changes, _) = apply(&fields, "en-GB");
        assert_eq!(
            tidied[0].1,
            "Analyses COLOUR and `analyze` at https://x.example/analyze for urn:ordin:a:skills:organize"
        );
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn numbers_group_only_when_standalone() {
        assert_eq!(numbers("about 12500 rows"), "about 12,500 rows");
        assert_eq!(numbers("ref A12500 and 12500.5"), "ref A12500 and 12500.5");
        assert_eq!(numbers("(1234567)."), "(1,234,567).");
    }

    #[test]
    fn dates_and_verbs() {
        let mut ambiguous = false;
        assert_eq!(dates("due 2026-02-29", &mut ambiguous), "due 2026-02-29");
        assert_eq!(
            dates("due 2028-02-29", &mut ambiguous),
            "due 29 February 2028"
        );
        assert!(!ambiguous);
        dates("on 3/4/26", &mut ambiguous);
        assert!(ambiguous);
        assert_eq!(
            verb_first("Comparing prices").as_deref(),
            Some("Compare prices")
        );
        assert_eq!(verb_first("checking").as_deref(), Some("Check"));
        assert_eq!(verb_first("Prices"), None);
        assert_eq!(group("100000"), "100,000");
    }
}
