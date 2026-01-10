//! The JSON Schema document of the whole Service API (ServiceApi.md *Schema and TypeScript
//! generation*; SA-API-02).

use ordin_types::prelude::EventKind;
use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use crate::api::{Effect, Scope};
use crate::blocks::{Block, BlockRejection, PlanToolArgs, ReportToolArgs};
use crate::catalogue::CATALOGUE;
use crate::events::ServiceEvent;
use crate::pairing::{PairReply, PairRequest};
use crate::wire::{ServiceApiVersion, ServiceError};

/// Every event kind of this version, in catalogue order.
pub(crate) const EVENT_KINDS: [EventKind; 21] = [
    EventKind::RunStateChanged,
    EventKind::RunProgress,
    EventKind::PlanChanged,
    EventKind::StepMarked,
    EventKind::StartUpReportReady,
    EventKind::ApprovalRequested,
    EventKind::ApprovalDecided,
    EventKind::CrewRoundChanged,
    EventKind::BlockShown,
    EventKind::Reset,
    EventKind::ServerStateChanged,
    EventKind::WorkspaceChanged,
    EventKind::NeedsYouChanged,
    EventKind::BlockAvailable,
    EventKind::LocaleChanged,
    EventKind::LanguageChanged,
    EventKind::BrandChanged,
    EventKind::SetupProgress,
    EventKind::SignInChanged,
    EventKind::KnowledgeAgentsChanged,
    EventKind::MoreFound,
];

fn scope_name(scope: Scope) -> &'static str {
    match scope {
        Scope::Any => "any",
        Scope::Local => "local",
    }
}

fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Read => "read",
        Effect::Change => "change",
    }
}

/// One JSON Schema document (draft 2020-12, as `schemars` 1 writes it) with `$defs` for every input,
/// result, event and block type, and the top-level `x-ordin-commands` and `x-ordin-events` arrays.
pub fn json_schemas() -> Value {
    let mut generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let commands: Vec<Value> = CATALOGUE
        .iter()
        .map(|meta| {
            json!({
                "name": meta.name,
                "ipc_name": meta.ipc_name,
                "rest_path": meta.rest_path,
                "scope": scope_name(meta.scope),
                "effect": effect_name(meta.effect),
                "input": (meta.input_schema)(&mut generator).to_value(),
                "result": (meta.result_schema)(&mut generator).to_value(),
            })
        })
        .collect();
    let event = generator.subschema_for::<ServiceEvent>().to_value();
    let events: Vec<Value> = EVENT_KINDS
        .iter()
        .map(|kind| json!({ "kind": kind.as_str(), "data": event }))
        .collect();
    // The block catalogue, the tool arguments and the envelope parts the screens read.
    generator.subschema_for::<Block>();
    generator.subschema_for::<PlanToolArgs>();
    generator.subschema_for::<ReportToolArgs>();
    generator.subschema_for::<BlockRejection>();
    generator.subschema_for::<ServiceError>();
    generator.subschema_for::<ServiceApiVersion>();
    generator.subschema_for::<PairRequest>();
    generator.subschema_for::<PairReply>();
    let defs = generator.take_definitions(true);
    let renames = generic_names(&defs);
    let defs: Map<String, Value> = defs
        .into_iter()
        .map(|(name, def)| (renames.get(&name).cloned().unwrap_or(name), def))
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect();
    let mut document = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": defs,
        "x-ordin-commands": commands,
        "x-ordin-events": events,
    });
    rewrite_refs(&mut document, &renames);
    document
}

/// The generic wrappers of `ordin-types` whose instances `schemars` names `Page`, `Page2`, … and the
/// property that carries their type parameter.
const GENERICS: [(&str, &str); 2] = [
    ("Page", "/properties/items/items"),
    ("Setting", "/properties/value"),
];

/// The name of a type parameter's schema: the type it refers to, or its JSON type.
fn parameter_name(schema: Option<&Value>) -> String {
    let Some(schema) = schema else {
        return "Value".to_owned();
    };
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        return reference.rsplit('/').next().unwrap_or(reference).to_owned();
    }
    match schema.get("type").and_then(Value::as_str) {
        Some(t) => {
            let mut chars = t.chars();
            chars
                .next()
                .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        }
        None => "Value".to_owned(),
    }
}

/// `Page2` → `Page_RemoteItem`: each instance of a generic wrapper is named after its parameter,
/// so the TypeScript names say what they hold (*chosen here*).
fn generic_names(defs: &Map<String, Value>) -> BTreeMap<String, String> {
    let mut renames = BTreeMap::new();
    for (name, def) in defs {
        for (base, pointer) in GENERICS {
            let numbered = name
                .strip_prefix(base)
                .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()));
            if numbered {
                renames.insert(
                    name.clone(),
                    format!("{base}_{}", parameter_name(def.pointer(pointer))),
                );
            }
        }
    }
    renames
}

/// Points every `$ref` at the renamed definitions.
fn rewrite_refs(value: &mut Value, renames: &BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if key == "$ref" {
                    if let Value::String(reference) = item {
                        if let Some(name) = reference.strip_prefix("#/$defs/") {
                            if let Some(new) = renames.get(name) {
                                *reference = format!("#/$defs/{new}");
                            }
                        }
                    }
                } else {
                    rewrite_refs(item, renames);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|i| rewrite_refs(i, renames)),
        _ => {}
    }
}
