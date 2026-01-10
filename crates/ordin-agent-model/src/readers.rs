//! The typed readers over a manifest's `body` and the shapes they return (AgentModel.md *Body shapes
//! read by the typed readers*, *Internal Logic* 7).
//!
//! Each reader returns `AM_FIELD_SHAPE_INVALID` when its key is present with the wrong shape, and the
//! empty value when the key is absent.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use ordin_types::domain::agents::{PartKind, VocabularyAxis};
use ordin_types::num::DecimalString;

use crate::error::AmError;
use crate::manifest::{scalar_text, Manifest};
use crate::value::YamlValue;
use crate::vocabulary::axis_from_str;

/// One conjunctive rule. Exactly one of class / ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RequiredPackRule {
    pub at_least: u32,
    pub class: Option<String>,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectablePack {
    pub id: String,
    pub description: Option<String>,
}

/// One entry of `fixedPracticePacks` (SA-NAME-07): a pack identifier, or a rule left for the
/// agent to ask (decision 64). Authored as a string, or as `{ask: {class: <c>}}` /
/// `{ask: {ids: [<id>, …]}}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixedPackEntry {
    Pack(String),
    Ask {
        class: Option<String>,
        ids: Vec<String>,
    },
}

/// `governanceMetadata.<axis>Overlays`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayOffer {
    pub available: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredLoopRule {
    pub required: bool,
    pub ids: Vec<String>,
}

/// The body of a ControlLoop part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ControlLoopSpec {
    pub strategy: String,
    pub trigger: String,
    pub requires: Vec<String>,
    pub modes: Vec<String>,
}

/// One entry of a ToolBinding's `bindings:`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolBindingSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub binding_type: String,
    pub permission: String,
    pub forbidden_flags: Vec<String>,
    pub coverage_floor: Option<DecimalString>,
    pub server_type: Option<String>,
    pub scope: Option<String>,
    pub env_ref: Option<String>,
    pub transport: Option<String>,
    /// `argumentPaths`: JSON Pointers to the arguments a rule may test (SA-CLOSE-02); empty when absent.
    #[serde(default)]
    pub argument_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackToolchain {
    pub forbidden_flags: Vec<String>,
    pub coverage_floor: Option<DecimalString>,
}

/// The memory contract of a Memory part (retains, excludes, retention, store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryContract {
    pub retains: Vec<String>,
    pub excludes: Vec<String>,
    pub max_age_days: Option<u32>,
    pub unbounded: bool,
    /// `local` or `remote`.
    pub store_location: String,
    pub memory_service: Option<String>,
}

/// One section of an OutputContract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSection {
    pub name: String,
    pub meaning: String,
    pub required: bool,
    /// `evidenceThreshold`; 0 when absent.
    pub evidence_threshold: u32,
    pub derived: bool,
}

/// The typed view of an OutputContract part's body (SA-CLOSE-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputContract {
    sections: Vec<ContractSection>,
}

/// One `appliesTo.workspaces` pin: an identifier and an exact (`1.0.0`) or caret (`^1.0.0`) range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliesTo {
    pub id: String,
    pub range: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyBody {
    pub axis: VocabularyAxis,
    pub terms: Vec<VocabularyTermBody>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyTermBody {
    pub code: String,
    pub label: String,
    pub description: String,
}

/// 08 *Lifecycle part*: the reference format's journey plus `stages`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyBody {
    pub seams: Vec<String>,
    pub trigger: Option<String>,
    pub stages: Vec<JourneyStage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyStage {
    pub id: String,
    pub name: String,
    pub role: String,
    pub agent: Option<String>,
    pub produces: Option<String>,
    pub handover_place_type: String,
    pub handover_place_target: String,
    pub answer_within_days: Option<u32>,
    /// None = the next stage in the list; Some(empty) = the lifecycle ends here.
    pub next: Option<Vec<StageNextEntry>>,
    pub together: bool,
    pub sends_back_to: Vec<SendsBackEntry>,
    /// `slackChannel` of an ordin-format stage; None when the key is absent. Amended January 2026 (ADR-102).
    pub slack_channel: Option<JourneySlackChannel>,
}

/// The Agent Model's own shape (it reads YAML only and does not validate Slack's identifier form;
/// the Catalogue Service and the Checker do, as `StageSlackChannel` in 2C). Amended January 2026 (ADR-102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneySlackChannel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageNextEntry {
    pub stage: String,
    pub when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendsBackEntry {
    pub stage: String,
    pub when: String,
    pub max_rounds: u32,
}

// ---- small readers over `YamlValue` ----

fn shape(key: &str, expected: &'static str) -> AmError {
    AmError::FieldShapeInvalid {
        key: key.to_owned(),
        expected,
    }
}

type Map = BTreeMap<String, YamlValue>;

/// `map[key]`, treating an explicit null as absent.
fn get<'a>(map: &'a Map, key: &str) -> Option<&'a YamlValue> {
    map.get(key).filter(|v| **v != YamlValue::Null)
}

fn as_map<'a>(value: &'a YamlValue, key: &str) -> Result<&'a Map, AmError> {
    value.as_map().ok_or_else(|| shape(key, "map"))
}

fn as_seq<'a>(value: &'a YamlValue, key: &str) -> Result<&'a [YamlValue], AmError> {
    value.as_seq().ok_or_else(|| shape(key, "list"))
}

/// A nested map at `path`; absent anywhere along the way is None.
fn nested<'a>(map: &'a Map, path: &[&str], key: &str) -> Result<Option<&'a YamlValue>, AmError> {
    let mut current = map;
    for (i, step) in path.iter().enumerate() {
        let Some(value) = get(current, step) else {
            return Ok(None);
        };
        if i + 1 == path.len() {
            return Ok(Some(value));
        }
        current = as_map(value, key)?;
    }
    Ok(None)
}

fn string(value: &YamlValue, key: &str) -> Result<String, AmError> {
    scalar_text(value).ok_or_else(|| shape(key, "string"))
}

fn opt_string(map: &Map, field: &str, key: &str) -> Result<Option<String>, AmError> {
    get(map, field).map(|v| string(v, key)).transpose()
}

fn string_or_empty(map: &Map, field: &str, key: &str) -> Result<String, AmError> {
    Ok(opt_string(map, field, key)?.unwrap_or_default())
}

fn strings(value: &YamlValue, key: &str) -> Result<Vec<String>, AmError> {
    as_seq(value, key)?
        .iter()
        .map(|item| scalar_text(item).ok_or_else(|| shape(key, "list of strings")))
        .collect()
}

fn opt_strings(map: &Map, field: &str, key: &str) -> Result<Vec<String>, AmError> {
    get(map, field)
        .map(|v| strings(v, key))
        .transpose()
        .map(Option::unwrap_or_default)
}

fn opt_bool(map: &Map, field: &str, key: &str) -> Result<bool, AmError> {
    match get(map, field) {
        None => Ok(false),
        Some(YamlValue::Bool(b)) => Ok(*b),
        Some(_) => Err(shape(key, "boolean")),
    }
}

fn opt_u32(map: &Map, field: &str, key: &str) -> Result<Option<u32>, AmError> {
    match get(map, field) {
        None => Ok(None),
        Some(YamlValue::Int(n)) => u32::try_from(*n)
            .map(Some)
            .map_err(|_| shape(key, "non-negative integer")),
        Some(_) => Err(shape(key, "non-negative integer")),
    }
}

fn decimal(value: &YamlValue, key: &str) -> Result<DecimalString, AmError> {
    let text = match value {
        YamlValue::Int(n) => n.to_string(),
        YamlValue::Decimal(text) => text.clone(),
        _ => return Err(shape(key, "decimal number")),
    };
    DecimalString::parse(&text).map_err(|_| shape(key, "decimal number"))
}

fn opt_decimal(map: &Map, field: &str, key: &str) -> Result<Option<DecimalString>, AmError> {
    get(map, field).map(|v| decimal(v, key)).transpose()
}

/// The `governanceMetadata` key of an overlay axis.
pub(crate) fn overlay_axis_key(axis: PartKind) -> Option<&'static str> {
    Some(match axis {
        PartKind::BrandOverlay => "brandOverlays",
        PartKind::JurisdictionOverlay => "jurisdictionOverlays",
        PartKind::LanguageOverlay => "languageOverlays",
        PartKind::StyleOverlay => "styleOverlays",
        PartKind::SourceWorkflowOverlay => "sourceWorkflowOverlays",
        PartKind::HandoffOverlay => "handoffOverlays",
        _ => return None,
    })
}

/// The `required<Axis>Overlays` key of an overlay axis.
pub(crate) fn required_overlay_key(axis: PartKind) -> Option<&'static str> {
    Some(match axis {
        PartKind::BrandOverlay => "requiredBrandOverlays",
        PartKind::JurisdictionOverlay => "requiredJurisdictionOverlays",
        PartKind::LanguageOverlay => "requiredLanguageOverlays",
        PartKind::StyleOverlay => "requiredStyleOverlays",
        PartKind::SourceWorkflowOverlay => "requiredSourceWorkflowOverlays",
        PartKind::HandoffOverlay => "requiredHandoffOverlays",
        _ => return None,
    })
}

/// The most argument paths one binding may name, and the length bounds of each (SA-CLOSE-02).
const MAX_ARGUMENT_PATHS: usize = 50;
const MIN_ARGUMENT_PATH_CHARS: usize = 2;
const MAX_ARGUMENT_PATH_CHARS: usize = 512;
/// The longest `argument-hint` (SA-CLOSE-01).
const MAX_ARGUMENT_HINT_CHARS: usize = 200;
/// The largest `evidenceThreshold` of a contract section (SA-CLOSE-03).
const MAX_EVIDENCE_THRESHOLD: i64 = 1_000;

/// One JSON Pointer (RFC 6901) with the two wildcard tokens of SA-CLOSE-02.
fn is_argument_path(path: &str) -> bool {
    let chars = path.chars().count();
    if !(MIN_ARGUMENT_PATH_CHARS..=MAX_ARGUMENT_PATH_CHARS).contains(&chars) {
        return false;
    }
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    let tokens: Vec<&str> = rest.split('/').collect();
    let last = tokens.len() - 1;
    tokens.iter().enumerate().all(|(i, token)| {
        if token.is_empty() || (*token == "*" && i != last) {
            return false;
        }
        let mut chars = token.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return false;
            }
        }
        true
    })
}

// spec: ordin-desktop/components/AgentModel#SA-CLOSE-02
fn argument_paths(value: Option<&YamlValue>) -> Result<Vec<String>, AmError> {
    let refuse = || shape("argumentPaths", "JSON Pointer list");
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value.as_seq().ok_or_else(refuse)?;
    if items.len() > MAX_ARGUMENT_PATHS {
        return Err(refuse());
    }
    items
        .iter()
        .map(|item| match item.as_str() {
            Some(path) if is_argument_path(path) => Ok(path.to_owned()),
            _ => Err(refuse()),
        })
        .collect()
}

fn read_binding(item: &YamlValue) -> Result<ToolBindingSpec, AmError> {
    let map = as_map(item, "bindings")?;
    Ok(ToolBindingSpec {
        name: string_or_empty(map, "name", "bindings.name")?,
        binding_type: string_or_empty(map, "type", "bindings.type")?,
        permission: string_or_empty(map, "permission", "bindings.permission")?,
        forbidden_flags: opt_strings(map, "forbiddenFlags", "bindings.forbiddenFlags")?,
        coverage_floor: opt_decimal(map, "coverageFloor", "bindings.coverageFloor")?,
        server_type: opt_string(map, "serverType", "bindings.serverType")?,
        scope: opt_string(map, "scope", "bindings.scope")?,
        env_ref: opt_string(map, "envRef", "bindings.envRef")?,
        transport: opt_string(map, "transport", "bindings.transport")?,
        argument_paths: argument_paths(get(map, "argumentPaths"))?,
    })
}

/// `^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$`, the version pattern.
pub(crate) fn is_version(text: &str) -> bool {
    let (core_and_pre, build) = match text.split_once('+') {
        Some((head, build)) => (head, Some(build)),
        None => (text, None),
    };
    let (core, pre) = match core_and_pre.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (core_and_pre, None),
    };
    let ident_ok = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    };
    let numbers: Vec<&str> = core.split('.').collect();
    numbers.len() == 3
        && numbers
            .iter()
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(ident_ok)
        && build.is_none_or(ident_ok)
}

fn applies_to_entry(text: &str) -> Result<AppliesTo, AmError> {
    let refuse = || shape("appliesTo", "identifier or identifier@range");
    // A legacy identifier starts with `@`, so the range separator is the last `@` after the first byte.
    match text.rfind('@').filter(|at| *at > 0) {
        None => Ok(AppliesTo {
            id: text.to_owned(),
            range: String::new(),
        }),
        Some(at) => {
            let (id, range) = (&text[..at], &text[at + 1..]);
            let exact = range.strip_prefix('^').unwrap_or(range);
            if id.is_empty() || !is_version(exact) {
                return Err(refuse());
            }
            Ok(AppliesTo {
                id: id.to_owned(),
                range: range.to_owned(),
            })
        }
    }
}

fn read_stage(item: &YamlValue) -> Result<JourneyStage, AmError> {
    let map = as_map(item, "stages")?;
    let next = match map.get("next") {
        None | Some(YamlValue::Null) => None,
        Some(value) => Some(
            as_seq(value, "next")?
                .iter()
                .map(|entry| {
                    let entry = as_map(entry, "next")?;
                    Ok(StageNextEntry {
                        stage: string_or_empty(entry, "stage", "next.stage")?,
                        when: opt_string(entry, "when", "next.when")?,
                    })
                })
                .collect::<Result<Vec<_>, AmError>>()?,
        ),
    };
    let sends_back_to = match get(map, "sendsBackTo") {
        None => Vec::new(),
        Some(value) => as_seq(value, "sendsBackTo")?
            .iter()
            .map(|entry| {
                let entry = as_map(entry, "sendsBackTo")?;
                Ok(SendsBackEntry {
                    stage: string_or_empty(entry, "stage", "sendsBackTo.stage")?,
                    when: string_or_empty(entry, "when", "sendsBackTo.when")?,
                    max_rounds: opt_u32(entry, "maxRounds", "sendsBackTo.maxRounds")?.unwrap_or(0),
                })
            })
            .collect::<Result<Vec<_>, AmError>>()?,
    };
    let handover = match get(map, "handoverPlace") {
        None => None,
        Some(value) => Some(as_map(value, "handoverPlace")?),
    };
    let handover_field = |field: &str| -> Result<String, AmError> {
        handover
            .map(|h| string_or_empty(h, field, &format!("handoverPlace.{field}")))
            .transpose()
            .map(Option::unwrap_or_default)
    };
    Ok(JourneyStage {
        id: string_or_empty(map, "id", "stages.id")?,
        name: string_or_empty(map, "name", "stages.name")?,
        role: string_or_empty(map, "role", "stages.role")?,
        agent: opt_string(map, "agent", "stages.agent")?,
        produces: opt_string(map, "produces", "stages.produces")?,
        handover_place_type: handover_field("type")?,
        handover_place_target: handover_field("target")?,
        answer_within_days: opt_u32(map, "answerWithinDays", "answerWithinDays")?,
        next,
        together: opt_bool(map, "together", "together")?,
        sends_back_to,
        slack_channel: slack_channel(map.get("slackChannel"))?,
    })
}

// spec: ordin-desktop/components/AgentModel#ADR-102
fn slack_channel(value: Option<&YamlValue>) -> Result<Option<JourneySlackChannel>, AmError> {
    let refuse = || shape("slackChannel", "a map with id and name");
    let Some(value) = value else {
        return Ok(None);
    };
    let map = value.as_map().ok_or_else(refuse)?;
    let field = |key: &str| match map.get(key).and_then(YamlValue::as_str) {
        Some(text) if !text.is_empty() => Ok(text.to_owned()),
        _ => Err(refuse()),
    };
    Ok(Some(JourneySlackChannel {
        id: field("id")?,
        name: field("name")?,
    }))
}

/// One pack rule of `requiredPracticePacks`.
fn read_pack_rule(item: &YamlValue) -> Result<RequiredPackRule, AmError> {
    const KEY: &str = "requiredPracticePacks";
    let map = as_map(item, KEY)?;
    // SPEC-GAP-6: the spec states `atLeast` as an integer of at least 1 but not what an absent
    // `atLeast` means; absent reads as 1 (one pack satisfies the rule).
    let at_least = match get(map, "atLeast") {
        None => 1,
        Some(YamlValue::Int(n)) if *n >= 1 => {
            u32::try_from(*n).map_err(|_| shape(KEY, "atLeast of at least 1"))?
        }
        Some(_) => return Err(shape(KEY, "atLeast of at least 1")),
    };
    let where_class = match get(map, "where") {
        None => None,
        Some(value) => Some(
            opt_string(as_map(value, KEY)?, "class", KEY)?
                .ok_or_else(|| shape(KEY, "where with a class"))?,
        ),
    };
    let class_key = opt_string(map, "class", KEY)?;
    let ids = get(map, "ids").map(|v| strings(v, KEY)).transpose()?;
    let any_of = get(map, "anyOf").map(|v| strings(v, KEY)).transpose()?;
    let classes: Vec<String> = where_class.into_iter().chain(class_key).collect();
    let id_lists: Vec<Vec<String>> = ids.into_iter().chain(any_of).collect();
    match (classes.len(), id_lists.len()) {
        (1, 0) => Ok(RequiredPackRule {
            at_least,
            class: classes.into_iter().next(),
            ids: Vec::new(),
        }),
        (0, 1) => Ok(RequiredPackRule {
            at_least,
            class: None,
            ids: id_lists.into_iter().next().unwrap_or_default(),
        }),
        _ => Err(shape(KEY, "exactly one of class or ids")),
    }
}

fn read_fixed_entry(item: &YamlValue) -> Result<FixedPackEntry, AmError> {
    const KEY: &str = "fixedPracticePacks";
    if let Some(id) = item.as_str() {
        return Ok(FixedPackEntry::Pack(id.to_owned()));
    }
    let map = as_map(item, KEY)?;
    let ask = match (map.len(), get(map, "ask")) {
        (1, Some(ask)) => as_map(ask, KEY)?,
        _ => return Err(shape(KEY, "identifier or ask")),
    };
    let class = opt_string(ask, "class", KEY)?;
    let ids = get(ask, "ids").map(|v| strings(v, KEY)).transpose()?;
    match (class, ids) {
        (Some(class), None) => Ok(FixedPackEntry::Ask {
            class: Some(class),
            ids: Vec::new(),
        }),
        (None, Some(ids)) => Ok(FixedPackEntry::Ask { class: None, ids }),
        _ => Err(shape(KEY, "ask with exactly one of class or ids")),
    }
}

impl Manifest {
    fn body_seq(&self, key: &str) -> Result<&[YamlValue], AmError> {
        match get(&self.body, key) {
            None => Ok(&[]),
            Some(value) => as_seq(value, key),
        }
    }

    pub fn required_practice_packs(&self) -> Result<Vec<RequiredPackRule>, AmError> {
        self.body_seq("requiredPracticePacks")?
            .iter()
            .map(read_pack_rule)
            .collect()
    }

    pub fn selectable_practice_packs(&self) -> Result<Vec<SelectablePack>, AmError> {
        const KEY: &str = "selectablePracticePacks";
        self.body_seq(KEY)?
            .iter()
            .map(|item| match item {
                YamlValue::Str(id) => Ok(SelectablePack {
                    id: id.clone(),
                    description: None,
                }),
                YamlValue::Map(map) => Ok(SelectablePack {
                    id: opt_string(map, "id", KEY)?.ok_or_else(|| shape(KEY, "map with an id"))?,
                    description: opt_string(map, "description", KEY)?,
                }),
                _ => Err(shape(KEY, "identifier or map with an id")),
            })
            .collect()
    }

    /// `fixedPracticePacks` (SA-NAME-07); absent reads as empty.
    pub fn fixed_practice_packs(&self) -> Result<Vec<FixedPackEntry>, AmError> {
        self.body_seq("fixedPracticePacks")?
            .iter()
            .map(read_fixed_entry)
            .collect()
    }

    /// `governanceMetadata.<axis>Overlays`; None for a kind that is not an overlay axis or when absent.
    pub fn overlay_offer(&self, axis: PartKind) -> Result<Option<OverlayOffer>, AmError> {
        let Some(axis_key) = overlay_axis_key(axis) else {
            return Ok(None);
        };
        let key = format!("governanceMetadata.{axis_key}");
        let Some(value) = nested(&self.body, &["governanceMetadata", axis_key], &key)? else {
            return Ok(None);
        };
        match value {
            YamlValue::Seq(_) => Ok(Some(OverlayOffer {
                available: strings(value, &key)?,
                default: None,
            })),
            YamlValue::Map(map) => Ok(Some(OverlayOffer {
                available: opt_strings(map, "available", &key)?,
                default: opt_string(map, "default", &key)?.filter(|d| !d.is_empty()),
            })),
            _ => Err(shape(&key, "list or map of available and default")),
        }
    }

    /// `required<Axis>Overlays: {anyOf: [...]}` for each axis that has the key, keyed by axis.
    pub fn required_overlays(
        &self,
    ) -> Result<std::collections::BTreeMap<PartKind, Vec<String>>, AmError> {
        let mut out = std::collections::BTreeMap::new();
        for axis in crate::kinds::overlay_order() {
            let Some(key) = required_overlay_key(*axis) else {
                continue;
            };
            if let Some(value) = get(&self.body, key) {
                let map = as_map(value, key)?;
                out.insert(*axis, opt_strings(map, "anyOf", key)?);
            }
        }
        Ok(out)
    }

    pub fn required_control_loop(&self) -> Result<Option<RequiredLoopRule>, AmError> {
        const KEY: &str = "requiredControlLoop";
        let Some(value) = get(&self.body, KEY) else {
            return Ok(None);
        };
        let map = as_map(value, KEY)?;
        Ok(Some(RequiredLoopRule {
            required: opt_bool(map, "required", KEY)?,
            ids: opt_strings(map, "ids", KEY)?,
        }))
    }

    /// `allowedOperatingModes`; items that are not strings are skipped.
    pub fn allowed_operating_modes(&self) -> Vec<String> {
        get(&self.body, "allowedOperatingModes")
            .and_then(YamlValue::as_seq)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn default_mode(&self) -> Option<String> {
        get(&self.body, "defaultMode")
            .and_then(YamlValue::as_str)
            .map(str::to_owned)
    }

    /// The body of a ControlLoop part; `strategy` absent → `Ok(None)`.
    pub fn control_loop(&self) -> Result<Option<ControlLoopSpec>, AmError> {
        let Some(strategy) = opt_string(&self.body, "strategy", "strategy")? else {
            return Ok(None);
        };
        Ok(Some(ControlLoopSpec {
            strategy,
            trigger: string_or_empty(&self.body, "trigger", "trigger")?,
            requires: opt_strings(&self.body, "requires", "requires")?,
            modes: opt_strings(&self.body, "modes", "modes")?,
        }))
    }

    /// `toolPermissions.workspaceOverrides.prohibited`.
    pub fn prohibited_verbs(&self) -> Result<Vec<String>, AmError> {
        const KEY: &str = "toolPermissions.workspaceOverrides.prohibited";
        nested(
            &self.body,
            &["toolPermissions", "workspaceOverrides", "prohibited"],
            KEY,
        )?
        .map(|v| strings(v, KEY))
        .transpose()
        .map(Option::unwrap_or_default)
    }

    /// `bindings` of a ToolBinding part.
    pub fn tool_bindings(&self) -> Result<Vec<ToolBindingSpec>, AmError> {
        self.body_seq("bindings")?
            .iter()
            .map(read_binding)
            .collect()
    }

    /// `defaultToolPolicy.requiresHumanApproval` of a Capability (ADR-090 item 1).
    pub fn requires_human_approval(&self) -> Result<Vec<String>, AmError> {
        const KEY: &str = "defaultToolPolicy.requiresHumanApproval";
        nested(
            &self.body,
            &["defaultToolPolicy", "requiresHumanApproval"],
            KEY,
        )?
        .map(|v| strings(v, KEY))
        .transpose()
        .map(Option::unwrap_or_default)
    }

    /// `toolchain.forbiddenFlags`, `toolchain.coverageFloor` of a PracticePack.
    pub fn pack_toolchain(&self) -> Result<Option<PackToolchain>, AmError> {
        let Some(value) = get(&self.body, "toolchain") else {
            return Ok(None);
        };
        let map = as_map(value, "toolchain")?;
        Ok(Some(PackToolchain {
            forbidden_flags: opt_strings(map, "forbiddenFlags", "toolchain.forbiddenFlags")?,
            coverage_floor: opt_decimal(map, "coverageFloor", "toolchain.coverageFloor")?,
        }))
    }

    /// `retains`, `excludes`, `retention`, `store` of a Memory part; None when none of them is present.
    // SPEC-GAP-8: the spec does not say what an absent `store.location` reads as; it reads as the
    // empty string, which the Checker can report, rather than as a chosen default.
    pub fn memory_contract(&self) -> Result<Option<MemoryContract>, AmError> {
        let present = ["retains", "excludes", "retention", "store"]
            .iter()
            .any(|key| get(&self.body, key).is_some());
        if !present {
            return Ok(None);
        }
        let retention = match get(&self.body, "retention") {
            None => None,
            Some(value) => Some(as_map(value, "retention")?),
        };
        let store = match get(&self.body, "store") {
            None => None,
            Some(value) => Some(as_map(value, "store")?),
        };
        Ok(Some(MemoryContract {
            retains: opt_strings(&self.body, "retains", "retains")?,
            excludes: opt_strings(&self.body, "excludes", "excludes")?,
            max_age_days: retention
                .map(|r| opt_u32(r, "maxAgeDays", "retention.maxAgeDays"))
                .transpose()?
                .flatten(),
            unbounded: retention
                .map(|r| opt_bool(r, "unbounded", "retention.unbounded"))
                .transpose()?
                .unwrap_or(false),
            store_location: store
                .map(|s| string_or_empty(s, "location", "store.location"))
                .transpose()?
                .unwrap_or_default(),
            memory_service: store
                .map(|s| opt_string(s, "memoryService", "store.memoryService"))
                .transpose()?
                .flatten(),
        }))
    }

    /// Command parts: the front-matter `argument-hint` string (SA-CLOSE-01).
    // spec: ordin-desktop/components/AgentModel#SA-CLOSE-01
    pub fn argument_hint(&self) -> Result<Option<String>, AmError> {
        match self.body.get("argument-hint") {
            None => Ok(None),
            Some(YamlValue::Str(hint)) if hint.chars().count() <= MAX_ARGUMENT_HINT_CHARS => {
                Ok(Some(hint.clone()))
            }
            Some(_) => Err(shape("argument-hint", "string up to 200 characters")),
        }
    }

    /// `governanceMetadata.publishGate` as a string.
    pub fn publish_gate(&self) -> Option<String> {
        get(&self.body, "governanceMetadata")
            .and_then(YamlValue::as_map)
            .and_then(|g| get(g, "publishGate"))
            .and_then(YamlValue::as_str)
            .map(str::to_owned)
    }

    /// `appliesTo.workspaces` as `<id>` or `<id>@<range>`.
    // SPEC-GAP-9: the spec does not give the range of an entry without `@`; it reads as the empty
    // range, which callers treat as any version.
    pub fn applies_to(&self) -> Result<Vec<AppliesTo>, AmError> {
        const KEY: &str = "appliesTo";
        let Some(value) = nested(&self.body, &["appliesTo", "workspaces"], KEY)? else {
            return Ok(Vec::new());
        };
        as_seq(value, KEY)?
            .iter()
            .map(|item| {
                item.as_str()
                    .ok_or_else(|| shape(KEY, "identifier or identifier@range"))
                    .and_then(applies_to_entry)
            })
            .collect()
    }

    /// `axis` and `terms[]` of a Vocabulary part; `axis` absent → `Ok(None)`.
    pub fn vocabulary(&self) -> Result<Option<VocabularyBody>, AmError> {
        let Some(axis) = get(&self.body, "axis") else {
            return Ok(None);
        };
        let axis = axis
            .as_str()
            .and_then(axis_from_str)
            .ok_or_else(|| shape("axis", "a vocabulary axis"))?;
        let terms = self
            .body_seq("terms")?
            .iter()
            .map(|item| {
                let map = as_map(item, "terms")?;
                Ok(VocabularyTermBody {
                    code: opt_string(map, "code", "terms.code")?
                        .ok_or_else(|| shape("terms.code", "string"))?,
                    label: string_or_empty(map, "label", "terms.label")?,
                    description: string_or_empty(map, "description", "terms.description")?,
                })
            })
            .collect::<Result<Vec<_>, AmError>>()?;
        Ok(Some(VocabularyBody { axis, terms }))
    }

    /// `seams`, `trigger` and `stages[]` of a Journey part; None when neither `seams` nor `stages`
    /// is present (`trigger` alone is also a ControlLoop key, so it does not make a journey).
    pub fn journey(&self) -> Result<Option<JourneyBody>, AmError> {
        let present = ["seams", "stages"]
            .iter()
            .any(|key| self.body.contains_key(*key));
        if !present {
            return Ok(None);
        }
        Ok(Some(JourneyBody {
            seams: opt_strings(&self.body, "seams", "seams")?,
            trigger: opt_string(&self.body, "trigger", "trigger")?,
            stages: self
                .body_seq("stages")?
                .iter()
                .map(read_stage)
                .collect::<Result<_, _>>()?,
        }))
    }
}

impl OutputContract {
    /// Reads `sections`; absent → no sections. Does not check the manifest's kind.
    // spec: ordin-desktop/components/AgentModel#SA-CLOSE-03
    pub fn read(manifest: &Manifest) -> Result<OutputContract, AmError> {
        let refuse = || shape("sections", "list of sections with a name");
        let Some(value) = get(&manifest.body, "sections") else {
            return Ok(OutputContract {
                sections: Vec::new(),
            });
        };
        let items = value.as_seq().ok_or_else(refuse)?;
        let sections = items
            .iter()
            .map(|item| {
                let map = item.as_map().ok_or_else(refuse)?;
                let name = match map.get("name") {
                    Some(YamlValue::Str(name)) if !name.is_empty() => name.clone(),
                    _ => return Err(refuse()),
                };
                let meaning = match get(map, "meaning") {
                    None => String::new(),
                    Some(YamlValue::Str(text)) => text.clone(),
                    Some(_) => return Err(refuse()),
                };
                let flag = |key: &str| match get(map, key) {
                    None => Ok(false),
                    Some(YamlValue::Bool(b)) => Ok(*b),
                    Some(_) => Err(refuse()),
                };
                let evidence_threshold = match get(map, "evidenceThreshold") {
                    None => 0,
                    Some(YamlValue::Int(n)) if (0..=MAX_EVIDENCE_THRESHOLD).contains(n) => {
                        u32::try_from(*n).map_err(|_| refuse())?
                    }
                    Some(_) => return Err(refuse()),
                };
                Ok(ContractSection {
                    name,
                    meaning,
                    required: flag("required")?,
                    evidence_threshold,
                    derived: flag("derived")?,
                })
            })
            .collect::<Result<Vec<_>, AmError>>()?;
        Ok(OutputContract { sections })
    }

    /// The sections in the contract's order.
    pub fn sections(&self) -> &[ContractSection] {
        &self.sections
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    fn manifest(body: &str) -> Manifest {
        let text = format!(
            "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: urn:ordin:a:workspaces:w\n{body}"
        );
        parse_manifest("w.yaml", text.as_bytes()).unwrap().unwrap()
    }

    fn shape_key(error: AmError) -> String {
        match error {
            AmError::FieldShapeInvalid { key, .. } => key,
            other => panic!("not a shape error: {other:?}"),
        }
    }

    #[test]
    fn pack_rules() {
        let m = manifest(
            "requiredPracticePacks:\n  - atLeast: 2\n    where: {class: stack}\n  - anyOf: [urn:ordin:a:practice-packs:x]\n  - ids: [urn:ordin:a:practice-packs:y]\n    atLeast: 1\n",
        );
        let rules = m.required_practice_packs().unwrap();
        assert_eq!(rules[0].at_least, 2);
        assert_eq!(rules[0].class.as_deref(), Some("stack"));
        assert_eq!(rules[1].at_least, 1);
        assert_eq!(
            rules[1].ids,
            vec!["urn:ordin:a:practice-packs:x".to_owned()]
        );
        for bad in [
            "requiredPracticePacks:\n  - atLeast: 0\n    class: x\n",
            "requiredPracticePacks:\n  - atLeast: 1\n",
            "requiredPracticePacks:\n  - class: x\n    where: {class: y}\n",
            "requiredPracticePacks: x\n",
            "requiredPracticePacks:\n  - where: [x]\n",
        ] {
            assert_eq!(
                shape_key(manifest(bad).required_practice_packs().unwrap_err()),
                "requiredPracticePacks",
                "{bad}"
            );
        }
        let m = manifest("selectablePracticePacks:\n  - urn:ordin:a:practice-packs:x\n  - id: urn:ordin:a:practice-packs:y\n    description: Why\n");
        let packs = m.selectable_practice_packs().unwrap();
        assert_eq!(packs[1].description.as_deref(), Some("Why"));
        assert!(manifest("selectablePracticePacks: [[x]]\n")
            .selectable_practice_packs()
            .is_err());
        let m = manifest("fixedPracticePacks:\n  - ask: {ids: [a, b]}\n");
        assert_eq!(
            m.fixed_practice_packs().unwrap(),
            vec![FixedPackEntry::Ask {
                class: None,
                ids: vec!["a".into(), "b".into()]
            }]
        );
        for bad in [
            "fixedPracticePacks:\n  - ask: {class: a, ids: [b]}\n",
            "fixedPracticePacks:\n  - other: x\n",
            "fixedPracticePacks:\n  - 3\n",
        ] {
            assert!(manifest(bad).fixed_practice_packs().is_err(), "{bad}");
        }
    }

    #[test]
    fn overlays_and_loops() {
        let m = manifest(
            "governanceMetadata:\n  styleOverlays: []\n  languageOverlays:\n    available: [en, fr]\n    default: ''\n  publishGate: review\nrequiredStyleOverlays: {anyOf: [s]}\nrequiredBrandOverlays: {anyOf: [b]}\nrequiredControlLoop: {required: true, ids: [l]}\nallowedOperatingModes: [m, 3]\ndefaultMode: m\n",
        );
        assert_eq!(
            m.overlay_offer(PartKind::StyleOverlay).unwrap(),
            Some(OverlayOffer {
                available: vec![],
                default: None
            })
        );
        assert_eq!(
            m.overlay_offer(PartKind::LanguageOverlay).unwrap(),
            Some(OverlayOffer {
                available: vec!["en".into(), "fr".into()],
                default: None
            })
        );
        assert_eq!(m.overlay_offer(PartKind::BrandOverlay).unwrap(), None);
        assert_eq!(m.overlay_offer(PartKind::Skill).unwrap(), None);
        assert_eq!(
            m.required_overlays().unwrap(),
            std::collections::BTreeMap::from([
                (PartKind::BrandOverlay, vec!["b".to_owned()]),
                (PartKind::StyleOverlay, vec!["s".to_owned()])
            ])
        );
        assert_eq!(
            m.required_control_loop().unwrap(),
            Some(RequiredLoopRule {
                required: true,
                ids: vec!["l".into()]
            })
        );
        assert_eq!(m.allowed_operating_modes(), vec!["m".to_owned()]);
        assert_eq!(m.default_mode().as_deref(), Some("m"));
        assert_eq!(m.publish_gate().as_deref(), Some("review"));
        assert!(manifest("governanceMetadata: x\n")
            .overlay_offer(PartKind::StyleOverlay)
            .is_err());
        assert!(manifest("governanceMetadata: {styleOverlays: 3}\n")
            .overlay_offer(PartKind::StyleOverlay)
            .is_err());
        assert!(manifest("requiredStyleOverlays: [x]\n")
            .required_overlays()
            .is_err());
        assert!(manifest("requiredControlLoop: {required: x}\n")
            .required_control_loop()
            .is_err());
    }

    #[test]
    fn bindings_and_paths() {
        assert!(is_argument_path("/to/[]/address"));
        assert!(is_argument_path("/attachments/*"));
        assert!(is_argument_path("/a~0b/c~1d"));
        for bad in ["to.address", "/", "/a//b", "/*/a", "/a~2", "/a~", ""] {
            assert!(!is_argument_path(bad), "{bad}");
        }
        assert!(!is_argument_path(&format!("/{}", "a".repeat(512))));
        let m = manifest(
            "bindings:\n  - name: git\n    type: cli\n    permission: write\n    forbiddenFlags: [--force]\n    coverageFloor: 0.80\n    serverType: stdio\n    scope: repo\n    envRef: GIT\n    transport: stdio\n",
        );
        let binding = &m.tool_bindings().unwrap()[0];
        assert_eq!(binding.coverage_floor, Some(DecimalString("0.80".into())));
        assert!(binding.argument_paths.is_empty());
        let many: Vec<String> = (0..51).map(|i| format!("/a{i}")).collect();
        let m = manifest(&format!(
            "bindings:\n  - name: x\n    argumentPaths: [{}]\n",
            many.join(", ")
        ));
        assert_eq!(shape_key(m.tool_bindings().unwrap_err()), "argumentPaths");
        assert!(
            manifest("bindings:\n  - name: x\n    coverageFloor: high\n")
                .tool_bindings()
                .is_err()
        );
        assert!(
            manifest("bindings:\n  - name: x\n    coverageFloor: 1.5e3\n")
                .tool_bindings()
                .is_err()
        );
    }

    #[test]
    fn capability_pack_memory() {
        let m = manifest(
            "defaultToolPolicy:\n  requiresHumanApproval: [git push]\ntoolchain:\n  forbiddenFlags: [--no-verify]\n  coverageFloor: 70\nretains: [decisions]\nexcludes: [secrets]\nretention: {maxAgeDays: 30}\nstore: {location: remote, memoryService: urn:ordin:a:tool-bindings:m}\ntoolPermissions:\n  workspaceOverrides:\n    prohibited: [deploy]\n",
        );
        assert_eq!(
            m.requires_human_approval().unwrap(),
            vec!["git push".to_owned()]
        );
        assert_eq!(
            m.pack_toolchain().unwrap(),
            Some(PackToolchain {
                forbidden_flags: vec!["--no-verify".into()],
                coverage_floor: Some(DecimalString("70".into()))
            })
        );
        let memory = m.memory_contract().unwrap().unwrap();
        assert_eq!(memory.max_age_days, Some(30));
        assert!(!memory.unbounded);
        assert_eq!(memory.store_location, "remote");
        assert_eq!(m.prohibited_verbs().unwrap(), vec!["deploy".to_owned()]);
        assert_eq!(manifest("").memory_contract().unwrap(), None);
        assert_eq!(manifest("").pack_toolchain().unwrap(), None);
        assert!(manifest("retention: {maxAgeDays: -1}\n")
            .memory_contract()
            .is_err());
        assert!(manifest("toolPermissions: {workspaceOverrides: x}\n")
            .prohibited_verbs()
            .is_err());
    }

    #[test]
    fn applies_vocabulary_journey() {
        let m = manifest(
            "appliesTo:\n  workspaces:\n    - urn:ordin:a:workspaces:w\n    - urn:ordin:a:workspaces:v@^1.2.0\n    - '@a/workspaces/x@1.0.0'\n",
        );
        let pins = m.applies_to().unwrap();
        assert_eq!(pins[0].range, "");
        assert_eq!(pins[1].range, "^1.2.0");
        assert_eq!(pins[2].id, "@a/workspaces/x");
        assert!(manifest("appliesTo:\n  workspaces: [x@latest]\n")
            .applies_to()
            .is_err());
        assert!(manifest("axis: colour\n").vocabulary().is_err());
        assert_eq!(manifest("").vocabulary().unwrap(), None);
        let m = manifest(
            "stages:\n  - id: a\n    next: []\n    sendsBackTo:\n      - {stage: b, when: unclear, maxRounds: 2}\n    handoverPlace: {type: folder, target: out}\n    answerWithinDays: 3\n    together: true\n",
        );
        let stage = &m.journey().unwrap().unwrap().stages[0];
        assert_eq!(stage.next, Some(vec![]));
        assert_eq!(stage.sends_back_to[0].max_rounds, 2);
        assert_eq!(stage.handover_place_target, "out");
        assert!(stage.together);
        assert!(manifest("stages:\n  - next: x\n").journey().is_err());
        assert!(manifest("stages:\n  - slackChannel: {id: '', name: x}\n")
            .journey()
            .is_err());
        assert!(is_version("1.2.3-rc.1+build.5"));
        assert!(!is_version("1.2"));
        assert!(!is_version("1.2.3-"));
    }

    #[test]
    fn contract_sections() {
        let m = manifest(
            "sections:\n  - name: a\n    meaning: m\n    required: true\n    evidenceThreshold: 2\n    derived: true\n  - name: b\n",
        );
        let contract = OutputContract::read(&m).unwrap();
        assert_eq!(contract.sections()[0].evidence_threshold, 2);
        assert!(!contract.sections()[1].required);
        for bad in [
            "sections:\n  - name: a\n    evidenceThreshold: 1001\n",
            "sections:\n  - name: a\n    required: yes\n",
            "sections:\n  - name: a\n    meaning: [x]\n",
            "sections: x\n",
        ] {
            assert_eq!(
                shape_key(OutputContract::read(&manifest(bad)).unwrap_err()),
                "sections"
            );
        }
        assert!(OutputContract::read(&manifest(""))
            .unwrap()
            .sections()
            .is_empty());
    }
}
