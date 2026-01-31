//! A fact's content as one subject–predicate–object triple (ADR-022): the checks the Service applies
//! before a write is queued and an import is admitted, the canonical form the store compares by, and
//! the text keyword search and the embedding read.
//!
//! Pure functions, no I/O. The Service Layer (C9) checks a triple with [`check_triple`]; the write
//! pipeline (C4) builds a fact's `TripleKey` from [`canonical_object`]; the stores write
//! [`triple_text`] as a triple's `content_text`.

use serde_json::Value as Json;

use crate::text::{collapse_ws, normalise_kind, normalise_predicate, predicate_words};
use crate::types::api::{Triple, TripleObject};
use crate::types::domain::ObjectKind;

/// The longest subject or entity name, in characters (SA-TRIPLE-01, SA-TRIPLE-02).
pub const NAME_MAX: usize = 512;
/// The longest predicate after `normalise_predicate`, in characters (SA-TRIPLE-01).
pub const PREDICATE_MAX: usize = 128;
/// The longest text value, in characters (SA-TRIPLE-02).
pub const TEXT_MAX: usize = 1000;
/// The longest unit, in characters (SA-TRIPLE-02).
pub const UNIT_MAX: usize = 32;

/// Parse `content` as a [`Triple`] and check every part (SA-TRIPLE-01, SA-TRIPLE-02). Returns the
/// triple with its subject trimmed and its predicate normalised, or the field the breach names:
/// `content` (not a triple at all), `content.subject`, `content.predicate` or `content.object`.
// spec: phase-1-analysis#SA-TRIPLE-01
pub fn check_triple(content: &Json) -> Result<Triple, &'static str> {
    let t: Triple = match serde_json::from_value(content.clone()) {
        Ok(t) => t,
        Err(_) => return Err(unparsed_field(content)),
    };
    let subject = collapse_ws(&t.subject);
    if !(1..=NAME_MAX).contains(&subject.chars().count()) {
        return Err("content.subject");
    }
    let predicate = normalise_predicate(&t.predicate);
    if !(1..=PREDICATE_MAX).contains(&predicate.chars().count()) {
        return Err("content.predicate");
    }
    if !object_is_valid(&t.object) {
        return Err("content.object");
    }
    Ok(Triple {
        subject,
        predicate,
        object: t.object,
    })
}

/// Whether `content` parses as a [`Triple`], without the bounds (an import's test, SA-TRIPLE-12).
pub fn parses_as_triple(content: &Json) -> bool {
    serde_json::from_value::<Triple>(content.clone()).is_ok()
}

/// The field a content that does not parse as a triple is refused under: `content.object` when the
/// three members are there with a text subject and predicate (so only the object is wrong), else
/// `content`.
fn unparsed_field(content: &Json) -> &'static str {
    let Json::Object(m) = content else {
        return "content";
    };
    let only_three = m
        .keys()
        .all(|k| k == "subject" || k == "predicate" || k == "object");
    let texts = m.get("subject").is_some_and(Json::is_string)
        && m.get("predicate").is_some_and(Json::is_string);
    if only_three && texts && m.contains_key("object") && m["object"].is_object() {
        "content.object"
    } else {
        "content"
    }
}

/// An object that does not read as its kind (SA-TRIPLE-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidObject;

/// Check one object by its kind (SA-TRIPLE-02): [`object_is_valid`] as a `Result`.
pub fn check_object(o: &TripleObject) -> Result<(), InvalidObject> {
    if object_is_valid(o) {
        Ok(())
    } else {
        Err(InvalidObject)
    }
}

/// Whether one object reads as its kind (SA-TRIPLE-02).
// spec: phase-1-analysis#SA-TRIPLE-02
pub fn object_is_valid(o: &TripleObject) -> bool {
    match o {
        TripleObject::Entity { name } => {
            (1..=NAME_MAX).contains(&collapse_ws(name).chars().count())
        }
        TripleObject::Text { value } => {
            let n = value.trim().chars().count();
            (1..=TEXT_MAX).contains(&n) && !value.chars().any(|c| c.is_control() && c != '\n')
        }
        TripleObject::Number { value, unit } => {
            is_decimal(value, 15, 6)
                && unit.as_deref().is_none_or(|u| {
                    let n = u.trim().chars().count();
                    (1..=UNIT_MAX).contains(&n) && !u.chars().any(char::is_control)
                })
        }
        TripleObject::Money { amount, currency } => {
            is_decimal(amount, 15, 4)
                && currency.len() == 3
                && currency.chars().all(|c| c.is_ascii_uppercase())
        }
        TripleObject::Date { value } => is_date(value),
        TripleObject::Boolean { .. } => true,
    }
}

/// `^-?[0-9]{1,int}(\.[0-9]{1,frac})?$`.
fn is_decimal(s: &str, int: usize, frac: usize) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (i, f) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let digits =
        |p: &str, max: usize| (1..=max).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit());
    digits(i, int) && f.is_none_or(|f| digits(f, frac))
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, a real calendar date.
fn is_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    let num = |p: &str, len: usize| p.len() == len && p.bytes().all(|b| b.is_ascii_digit());
    match parts.as_slice() {
        [y] => num(y, 4),
        [y, m] => num(y, 4) && num(m, 2) && (1..=12).contains(&m.parse::<u32>().unwrap_or(0)),
        [y, m, d] => {
            num(y, 4)
                && num(m, 2)
                && num(d, 2)
                && chrono::NaiveDate::from_ymd_opt(
                    y.parse().unwrap_or(0),
                    m.parse().unwrap_or(0),
                    d.parse().unwrap_or(0),
                )
                .is_some()
        }
        _ => false,
    }
}

/// A decimal in canonical form: trailing zeros after the point and a trailing point removed, `-0`
/// read as `0` (SA-TRIPLE-02).
fn canonical_decimal(s: &str) -> String {
    let mut out = s.to_string();
    if out.contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    if out
        .strip_prefix('-')
        .is_some_and(|r| r.bytes().all(|b| b == b'0'))
    {
        out = out.trim_start_matches('-').to_string();
    }
    out
}

/// The object's kind, its canonical value (`None` for an entity, whose value is its entity id, set by
/// the write pipeline after resolution) and its normalised unit or currency (SA-TRIPLE-02).
// spec: phase-1-analysis#SA-TRIPLE-02
pub fn canonical_object(o: &TripleObject) -> (ObjectKind, Option<String>, Option<String>) {
    match o {
        TripleObject::Entity { .. } => (ObjectKind::Entity, None, None),
        TripleObject::Text { value } => (ObjectKind::Text, Some(collapse_ws(value)), None),
        TripleObject::Number { value, unit } => (
            ObjectKind::Number,
            Some(canonical_decimal(value)),
            unit.as_deref().map(normalise_kind),
        ),
        TripleObject::Money { amount, currency } => (
            ObjectKind::Money,
            Some(canonical_decimal(amount)),
            Some(currency.clone()),
        ),
        TripleObject::Date { value } => (ObjectKind::Date, Some(value.clone()), None),
        TripleObject::Boolean { value } => (
            ObjectKind::Boolean,
            Some(if *value { "true" } else { "false" }.to_string()),
            None,
        ),
    }
}

/// A triple's text for keyword search and the embedding (SA-TRIPLE-03):
/// `<subject> <predicate words> <object text>[ <unit or currency>]`.
// spec: phase-1-analysis#SA-TRIPLE-03
pub fn triple_text(t: &Triple) -> String {
    let (object, unit) = match &t.object {
        TripleObject::Entity { name } => (name.clone(), None),
        TripleObject::Text { value } => (value.clone(), None),
        TripleObject::Number { value, unit } => (value.clone(), unit.clone()),
        TripleObject::Money { amount, currency } => (amount.clone(), Some(currency.clone())),
        TripleObject::Date { value } => (value.clone(), None),
        TripleObject::Boolean { value } => (value.to_string(), None),
    };
    let mut out = format!("{} {} {}", t.subject, predicate_words(&t.predicate), object);
    if let Some(u) = unit {
        out.push(' ');
        out.push_str(&u);
    }
    collapse_ws(&out)
}

/// The entity name an entity object names, if any.
pub fn object_name(t: &Triple) -> Option<&str> {
    match &t.object {
        TripleObject::Entity { name } => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_valid_triple_is_returned_trimmed_and_normalised() {
        let t = check_triple(&json!({
            "subject": "  Kestrel ",
            "predicate": " account   Manager",
            "object": {"kind": "entity", "name": "Tom Reid"}
        }))
        .unwrap();
        assert_eq!(t.subject, "Kestrel");
        assert_eq!(t.predicate, "account Manager");
    }

    /// The service-layer scenario outline and the bounds of SA-TRIPLE-01 / 02.
    #[test]
    fn content_that_is_not_one_triple_names_its_field() {
        let cases = [
            (
                json!({"statement": "Borealis supplies packaging"}),
                "content",
            ),
            (json!("text"), "content"),
            (
                json!({"subject": "", "predicate": "owns", "object": {"kind": "text", "value": "x"}}),
                "content.subject",
            ),
            (
                json!({"subject": "x".repeat(513), "predicate": "owns", "object": {"kind": "text", "value": "x"}}),
                "content.subject",
            ),
            (
                json!({"subject": "A", "predicate": "  ", "object": {"kind": "text", "value": "x"}}),
                "content.predicate",
            ),
            (
                json!({"subject": "A", "predicate": "p".repeat(129), "object": {"kind": "text", "value": "x"}}),
                "content.predicate",
            ),
            (
                json!({"subject": "Order 7731", "predicate": "price", "object": {"kind": "money", "amount": "12.5", "currency": "gbp"}}),
                "content.object",
            ),
            (
                json!({"subject": "Kestrel", "predicate": "foundingDate", "object": {"kind": "date", "value": "1998-02-30"}}),
                "content.object",
            ),
            (
                json!({"subject": "A", "predicate": "p", "object": {"kind": "colour", "value": "red"}}),
                "content.object",
            ),
            (
                json!({"subject": "A", "predicate": "p", "object": {"kind": "text", "value": "a\u{7}b"}}),
                "content.object",
            ),
            (
                json!({"subject": "A", "predicate": "p", "object": "B", "extra": 1}),
                "content",
            ),
        ];
        for (content, field) in cases {
            assert_eq!(check_triple(&content).unwrap_err(), field, "{content}");
        }
    }

    #[test]
    fn values_are_read_by_their_kind() {
        let ok = [
            json!({"kind": "number", "value": "-12.000001", "unit": "kg"}),
            json!({"kind": "number", "value": "123456789012345"}),
            json!({"kind": "money", "amount": "12.50", "currency": "GBP"}),
            json!({"kind": "date", "value": "2024"}),
            json!({"kind": "date", "value": "2024-02"}),
            json!({"kind": "date", "value": "2024-02-29"}),
            json!({"kind": "boolean", "value": true}),
            json!({"kind": "text", "value": "two\nlines"}),
        ];
        for o in ok {
            let o: TripleObject = serde_json::from_value(o.clone()).unwrap();
            assert!(object_is_valid(&o), "{o:?}");
        }
        let bad = [
            json!({"kind": "number", "value": "1.2345678"}),
            json!({"kind": "number", "value": "1e3"}),
            json!({"kind": "number", "value": "1", "unit": ""}),
            json!({"kind": "money", "amount": "1.23456", "currency": "GBP"}),
            json!({"kind": "money", "amount": "1", "currency": "GB"}),
            json!({"kind": "date", "value": "2023-02-29"}),
            json!({"kind": "date", "value": "2024-13"}),
            json!({"kind": "date", "value": "24"}),
            json!({"kind": "text", "value": ""}),
            json!({"kind": "entity", "name": " "}),
        ];
        for o in bad {
            let o: TripleObject = serde_json::from_value(o.clone()).unwrap();
            assert!(!object_is_valid(&o), "{o:?}");
        }
    }

    #[test]
    fn canonical_values_compare_by_form() {
        let money: TripleObject =
            serde_json::from_value(json!({"kind": "money", "amount": "12.50", "currency": "GBP"}))
                .unwrap();
        assert_eq!(
            canonical_object(&money),
            (ObjectKind::Money, Some("12.5".into()), Some("GBP".into()))
        );
        let cases = [
            ("10.00", "10"),
            ("-0", "0"),
            ("-0.0", "0"),
            ("7", "7"),
            ("1.50", "1.5"),
        ];
        for (raw, want) in cases {
            assert_eq!(canonical_decimal(raw), want, "{raw}");
        }
        let n: TripleObject = serde_json::from_value(
            json!({"kind": "number", "value": "3", "unit": " Staff  Members"}),
        )
        .unwrap();
        assert_eq!(
            canonical_object(&n),
            (
                ObjectKind::Number,
                Some("3".into()),
                Some("staff members".into())
            )
        );
        let t: TripleObject =
            serde_json::from_value(json!({"kind": "text", "value": "  Head  of sales "})).unwrap();
        assert_eq!(canonical_object(&t).1.as_deref(), Some("Head of sales"));
    }

    #[test]
    fn a_triples_text_splits_its_predicate() {
        let t = check_triple(&json!({
            "subject": "Kestrel",
            "predicate": "accountManager",
            "object": {"kind": "entity", "name": "Tom Reid"}
        }))
        .unwrap();
        assert_eq!(triple_text(&t), "Kestrel account Manager Tom Reid");
        let m = check_triple(&json!({
            "subject": "Order 7731",
            "predicate": "price",
            "object": {"kind": "money", "amount": "12.50", "currency": "GBP"}
        }))
        .unwrap();
        assert_eq!(triple_text(&m), "Order 7731 price 12.50 GBP");
    }
}
