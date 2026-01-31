//! Small readers over a manifest's untyped body, for the keys the Agent Model gives no typed reader
//! (assay policy, guideline target, crew members, vocabulary term state, handoff `consumes`).

use std::collections::BTreeMap;

use ordin_agent_model::{Manifest, YamlValue};

pub(crate) type Map = BTreeMap<String, YamlValue>;

/// `map[key]`, an explicit null read as absent.
pub(crate) fn get<'a>(map: &'a Map, key: &str) -> Option<&'a YamlValue> {
    match map.get(key) {
        None | Some(YamlValue::Null) => None,
        Some(value) => Some(value),
    }
}

pub(crate) fn as_map(value: &YamlValue) -> Option<&Map> {
    match value {
        YamlValue::Map(map) => Some(map),
        _ => None,
    }
}

pub(crate) fn as_seq(value: &YamlValue) -> Option<&[YamlValue]> {
    match value {
        YamlValue::Seq(items) => Some(items),
        _ => None,
    }
}

/// A scalar as text: strings, and numbers kept as their source text.
pub(crate) fn text(value: &YamlValue) -> Option<String> {
    match value {
        YamlValue::Str(s) | YamlValue::Decimal(s) => Some(s.clone()),
        YamlValue::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn str_at(map: &Map, key: &str) -> Option<String> {
    get(map, key).and_then(text)
}

/// A string, or a list of strings, under `key` of the body; anything else reads as empty.
pub(crate) fn strings(manifest: &Manifest, key: &str) -> Vec<String> {
    match get(&manifest.body, key) {
        Some(YamlValue::Seq(items)) => items.iter().filter_map(text).collect(),
        Some(value) => text(value).into_iter().collect(),
        None => Vec::new(),
    }
}
