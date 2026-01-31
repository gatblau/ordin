//! Argument rules (RulesEngine.md *Internal Logic* D.5.8 and D.7.1; wrightrun section G.4).

use std::collections::HashMap;

use regex::Regex;
use serde_json::Value;

use crate::address;
use crate::load::compile;
use crate::types::Address;
use crate::vocab::{AddressCondition, ArgumentForm, ArgumentRule};

/// One leaf of a walked place.
#[derive(Debug, PartialEq)]
enum Leaf {
    Text(String),
    /// A place or leaf that cannot be read: it holds against the call.
    Unreadable,
    Missing,
}

/// Walks a place: `.`-separated names, `[]` iterating a list.
fn walk(value: &Value, segments: &[&str], out: &mut Vec<Leaf>) {
    let Some((segment, rest)) = segments.split_first() else {
        out.push(match value {
            Value::String(text) => Leaf::Text(text.clone()),
            Value::Number(n) => Leaf::Text(n.to_string()),
            Value::Bool(b) => Leaf::Text(b.to_string()),
            _ => Leaf::Unreadable,
        });
        return;
    };
    let Value::Object(map) = value else {
        out.push(Leaf::Unreadable);
        return;
    };
    match segment.strip_suffix("[]") {
        Some(name) => match map.get(name) {
            None => out.push(Leaf::Missing),
            Some(Value::Array(items)) => {
                for item in items {
                    walk(item, rest, out);
                }
            }
            Some(_) => out.push(Leaf::Unreadable),
        },
        None => match map.get(*segment) {
            None => out.push(Leaf::Missing),
            Some(child) => walk(child, rest, out),
        },
    }
}

fn has_test(rule: &ArgumentRule) -> bool {
    !rule.equals_any.is_empty()
        || !rule.patterns.is_empty()
        || rule.max_length.is_some()
        || rule.address.is_some()
}

/// The test of one leaf: every stated condition holds. `Err` = an address the parser refuses.
fn test(rule: &ArgumentRule, leaf: &str, patterns: &HashMap<String, Regex>) -> Result<bool, ()> {
    if !rule.equals_any.is_empty() && !rule.equals_any.iter().any(|v| v == leaf) {
        return Ok(false);
    }
    if !rule.patterns.is_empty() {
        let mut any = false;
        for pattern in &rule.patterns {
            let matched = match patterns.get(pattern) {
                Some(regex) => regex.is_match(leaf),
                None => compile(pattern).ok_or(())?.is_match(leaf),
            };
            any |= matched;
        }
        if !any {
            return Ok(false);
        }
    }
    if let Some(max) = rule.max_length {
        if leaf.chars().count() > usize::try_from(max).unwrap_or(usize::MAX) {
            return Ok(false);
        }
    }
    if let Some(condition) = &rule.address {
        let parsed = address::parse(leaf).map_err(|_| ())?;
        if !address_holds(condition, &parsed) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn address_holds(condition: &AddressCondition, address: &Address) -> bool {
    if !condition.schemes.is_empty()
        && !condition
            .schemes
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&address.scheme))
    {
        return false;
    }
    if !condition.hosts.is_empty() {
        let host_ok = condition.hosts.iter().any(|host| {
            let host = host.to_lowercase();
            address.host == host
                || (condition.with_subdomains && address.host.ends_with(&format!(".{host}")))
        });
        if !host_ok {
            return false;
        }
    }
    if condition
        .host_is_ip
        .is_some_and(|flag| flag != address.host_is_ip)
    {
        return false;
    }
    if condition
        .host_is_single_label
        .is_some_and(|flag| flag != address.host_is_single_label)
    {
        return false;
    }
    if !condition.ports.is_empty() && !address.port.is_some_and(|p| condition.ports.contains(&p)) {
        return false;
    }
    if !condition.path_prefixes.is_empty() {
        let path_ok = condition.path_prefixes.iter().any(|prefix| {
            address.path == *prefix
                || if prefix.ends_with('/') {
                    address.path.starts_with(prefix.as_str())
                } else {
                    address.path.starts_with(&format!("{prefix}/"))
                }
        });
        if !path_ok {
            return false;
        }
    }
    true
}

/// Whether the rule holds against the call (refuse or pause).
pub(crate) fn holds(
    rule: &ArgumentRule,
    arguments: &Value,
    patterns: &HashMap<String, Regex>,
) -> bool {
    let segments: Vec<&str> = rule.argument.split('.').collect();
    let mut leaves = Vec::new();
    walk(arguments, &segments, &mut leaves);
    // An empty list passes.
    if leaves.is_empty() {
        return false;
    }
    if leaves.contains(&Leaf::Unreadable) {
        return true;
    }
    let texts: Vec<&str> = leaves
        .iter()
        .filter_map(|leaf| match leaf {
            Leaf::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let absent = texts.is_empty();
    match rule.form {
        ArgumentForm::RefuseWhen => {
            if absent {
                return rule.present == Some(false);
            }
            has_test(rule)
                && texts
                    .iter()
                    .any(|leaf| test(rule, leaf, patterns).unwrap_or(true))
        }
        ArgumentForm::RefuseUnless => {
            if rule.present == Some(false) {
                return !absent;
            }
            if absent {
                return true;
            }
            texts
                .iter()
                .any(|leaf| !test(rule, leaf, patterns).unwrap_or(false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocab::ArgumentOutcome;
    use serde_json::json;

    fn rule(form: ArgumentForm, argument: &str) -> ArgumentRule {
        ArgumentRule {
            name: "r".into(),
            tool: "t".into(),
            argument: argument.into(),
            form,
            outcome: ArgumentOutcome::Refuse,
            present: None,
            equals_any: Vec::new(),
            patterns: Vec::new(),
            max_length: None,
            address: None,
        }
    }

    #[test]
    fn places_walk_lists_and_objects() {
        let mut r = rule(ArgumentForm::RefuseWhen, "items[].url");
        r.patterns = vec!["^http:".into()];
        let none = HashMap::new();
        assert!(holds(&r, &json!({"items": [{"url": "http://x"}]}), &none));
        assert!(!holds(&r, &json!({"items": [{"url": "https://x"}]}), &none));
        assert!(!holds(&r, &json!({"items": []}), &none));
        assert!(holds(&r, &json!({"items": "not a list"}), &none));
        assert!(holds(&r, &json!({"items": [{"url": {"deep": 1}}]}), &none));
        assert!(holds(&r, &json!("not an object"), &none));
        assert!(!holds(&r, &json!({}), &none));
    }

    #[test]
    fn present_flags() {
        let mut when = rule(ArgumentForm::RefuseWhen, "a");
        when.present = Some(false);
        let none = HashMap::new();
        assert!(holds(&when, &json!({}), &none));
        assert!(!holds(&when, &json!({"a": "x"}), &none));
        let mut unless = rule(ArgumentForm::RefuseUnless, "a");
        assert!(holds(&unless, &json!({}), &none));
        assert!(!holds(&unless, &json!({"a": 3}), &none));
        unless.present = Some(false);
        assert!(holds(&unless, &json!({"a": true}), &none));
        assert!(!holds(&unless, &json!({}), &none));
    }

    #[test]
    fn leaf_tests() {
        let none = HashMap::new();
        let mut r = rule(ArgumentForm::RefuseUnless, "channel");
        r.equals_any = vec!["#team".into()];
        assert!(holds(&r, &json!({"channel": "#general"}), &none));
        assert!(!holds(&r, &json!({"channel": "#team"}), &none));
        let mut m = rule(ArgumentForm::RefuseWhen, "body");
        m.max_length = Some(3);
        assert!(!holds(&m, &json!({"body": "abcd"}), &none));
        let mut u = rule(ArgumentForm::RefuseUnless, "body");
        u.max_length = Some(3);
        assert!(holds(&u, &json!({"body": "abcd"}), &none));
        let mut bad = rule(ArgumentForm::RefuseWhen, "p");
        bad.patterns = vec!["(".into()];
        assert!(holds(&bad, &json!({"p": "x"}), &none));
    }

    #[test]
    fn address_conditions() {
        let none = HashMap::new();
        let mut r = rule(ArgumentForm::RefuseUnless, "url");
        r.address = Some(AddressCondition {
            schemes: vec!["https".into()],
            hosts: vec!["example.org".into()],
            with_subdomains: true,
            host_is_ip: Some(false),
            host_is_single_label: Some(false),
            ports: vec![443],
            path_prefixes: vec!["/api/public/".into(), "/docs".into()],
        });
        assert!(holds(
            &r,
            &json!({"url": "https://example.org.evil.net/api/public/x"}),
            &none
        ));
        assert!(!holds(
            &r,
            &json!({"url": "https://api.example.org/api/public/rates"}),
            &none
        ));
        assert!(!holds(
            &r,
            &json!({"url": "https://example.org/docs"}),
            &none
        ));
        assert!(!holds(
            &r,
            &json!({"url": "https://example.org/docs/a"}),
            &none
        ));
        assert!(holds(
            &r,
            &json!({"url": "https://example.org/docsx"}),
            &none
        ));
        assert!(holds(&r, &json!({"url": "http://example.org/docs"}), &none));
        assert!(holds(
            &r,
            &json!({"url": "https://example.org:8443/docs"}),
            &none
        ));
        assert!(holds(
            &r,
            &json!({"url": "https://user@example.org/docs"}),
            &none
        ));
        let mut ip = rule(ArgumentForm::RefuseWhen, "url");
        ip.address = Some(AddressCondition {
            host_is_ip: Some(true),
            ..AddressCondition::default()
        });
        assert!(holds(&ip, &json!({"url": "http://10.0.0.1/"}), &none));
        assert!(!holds(&ip, &json!({"url": "http://intranet/"}), &none));
        let mut single = rule(ArgumentForm::RefuseWhen, "url");
        single.address = Some(AddressCondition {
            host_is_single_label: Some(true),
            ..AddressCondition::default()
        });
        assert!(holds(&single, &json!({"url": "http://intranet/"}), &none));
        assert!(holds(&single, &json!({"url": "not an address"}), &none));
    }
}
