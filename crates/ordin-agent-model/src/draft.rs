//! The draft model: parts from the fields a person or the agent maker writes, and fields back from
//! parts (AgentModel.md *The draft model*, *Internal Logic* 14–18). Every method is pure.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value};

use ordin_types::domain::agents::{AgentDraft, PartKind};

use crate::archive::content_digest;
use crate::error::AmError;
use crate::ident::{is_part_identifier, matches, mint_identifier, Urn, PUBLISHER};
use crate::kinds::{kebab_kind_name, kind_info, FileConvention};
use crate::manifest::{markdown_body, Audience, Manifest, Metadata, UseCase, TYPED_METADATA_KEYS};
use crate::render::{part_file_path, render_part};
use crate::tree::{ModelPart, PartFiles};
use crate::value::{is_decimal_text, YamlValue};
use crate::API_VERSION;

/// A part the Foundry proposed for an agent draft (ADR-088 step 3, ADR-090 §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GeneratedPart {
    pub kind: PartKind,
    /// Plain name; the identifier is minted from it with `mint_identifier`.
    pub name: String,
    /// The kind's fields in the `fields_of` shape; `metadata.id` may be absent (it is minted).
    pub fields: serde_json::Value,
}

/// A part the agent draft reuses from the library, as the Authoring Service resolved it from
/// `AgentDraft.reused_parts` through the Catalogue Service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusedPart {
    pub kind: PartKind,
    pub identifier: String,
}

pub trait DraftModel: Send + Sync {
    /// Every part of an agent draft: one WorkspaceProfile, one SystemRole, and one part per
    /// `generated` entry, with the WorkspaceProfile referencing the generated and reused parts.
    /// `publisher` is the target repository's publisher directory without `@`.
    fn agent_parts_from_fields(
        &self,
        draft: &AgentDraft,
        publisher: &str,
        reused: &[ReusedPart],
        generated: &[GeneratedPart],
    ) -> Result<Vec<ModelPart>, AmError>;
    /// One part from its fields (the `fields_of` shape). `metadata.id` is required.
    fn part_from_fields(
        &self,
        kind: PartKind,
        fields: &serde_json::Value,
    ) -> Result<ModelPart, AmError>;
    /// The fields of one part, lossless: `part_from_fields(kind, &fields_of(p))` renders the
    /// same bytes as `p`'s manifest file.
    fn fields_of(&self, part: &ModelPart) -> serde_json::Value;
    /// JSON Schema (draft 2020-12) of the kind's fields, for the part-draft form.
    fn form(&self, kind: PartKind) -> serde_json::Value;
    /// A new part of `kind`: version 1.0.0, status draft, empty body.
    fn scaffold(&self, kind: PartKind, name: &str, identifier: &str) -> Result<ModelPart, AmError>;
    /// The agent draft as a Markdown write-up with the `role-dossier` section headings.
    fn render_write_up(&self, draft: &AgentDraft) -> String;
}

/// The implementation of `DraftModel`. Stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct FieldModel;

const DECIMAL_KEY: &str = "$decimal";
const NUMBER_EXPECTED: &str = "integer or {\"$decimal\": text}";

fn fields_error(field: &str, expected: &'static str) -> AmError {
    AmError::DraftFieldsInvalid {
        field: field.to_owned(),
        expected,
    }
}

// ---- YamlValue <-> JSON (step 14) ----

fn json_of(value: &YamlValue) -> Value {
    match value {
        YamlValue::Null => Value::Null,
        YamlValue::Bool(b) => Value::Bool(*b),
        YamlValue::Int(n) => Value::from(*n),
        YamlValue::Decimal(text) => json!({ DECIMAL_KEY: text }),
        YamlValue::Str(text) => Value::String(text.clone()),
        YamlValue::Seq(items) => Value::Array(items.iter().map(json_of).collect()),
        YamlValue::Map(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), json_of(v))).collect())
        }
    }
}

fn yaml_of(value: &Value, field: &str) -> Result<YamlValue, AmError> {
    Ok(match value {
        Value::Null => YamlValue::Null,
        Value::Bool(b) => YamlValue::Bool(*b),
        Value::Number(n) => YamlValue::Int(
            n.as_i64()
                .ok_or_else(|| fields_error(field, NUMBER_EXPECTED))?,
        ),
        Value::String(text) => YamlValue::Str(text.clone()),
        Value::Array(items) => YamlValue::Seq(
            items
                .iter()
                .map(|item| yaml_of(item, field))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(decimal) = map.get(DECIMAL_KEY) {
                    return match decimal.as_str() {
                        Some(text) if is_decimal_text(text) => {
                            Ok(YamlValue::Decimal(text.to_owned()))
                        }
                        _ => Err(fields_error(field, NUMBER_EXPECTED)),
                    };
                }
            }
            let mut out = BTreeMap::new();
            for (key, item) in map {
                out.insert(key.clone(), yaml_of(item, &format!("{field}.{key}"))?);
            }
            YamlValue::Map(out)
        }
    })
}

// ---- fields_of / part_from_fields ----

fn strings_json(items: &[String]) -> Value {
    Value::Array(items.iter().map(|s| Value::String(s.clone())).collect())
}

fn metadata_json(metadata: &Metadata) -> Value {
    let mut map = JsonMap::new();
    let text = |s: &str| Value::String(s.to_owned());
    map.insert("id".into(), text(&metadata.id));
    map.insert("name".into(), text(&metadata.name));
    map.insert("version".into(), text(&metadata.version));
    map.insert("owner".into(), text(&metadata.owner));
    if let Some(loading) = &metadata.loading {
        map.insert("loading".into(), text(loading));
    }
    map.insert("description".into(), text(&metadata.description));
    map.insert("category".into(), text(&metadata.category));
    map.insert("class".into(), text(&metadata.class));
    map.insert(
        "audience".into(),
        json!({
            "domains": strings_json(&metadata.audience.domains),
            "roles": strings_json(&metadata.audience.roles),
            "languages": strings_json(&metadata.audience.languages),
        }),
    );
    map.insert(
        "useCases".into(),
        Value::Array(
            metadata
                .use_cases
                .iter()
                .map(|c| json!({ "title": c.title, "summary": c.summary }))
                .collect(),
        ),
    );
    if let Some(status) = &metadata.status {
        map.insert("status".into(), text(status));
    }
    if let Some(supersedes) = &metadata.supersedes {
        map.insert("supersedes".into(), text(supersedes));
    }
    for (key, value) in &metadata.extra {
        map.insert(key.clone(), json_of(value));
    }
    Value::Object(map)
}

fn json_string(map: &JsonMap<String, Value>, key: &str) -> Result<Option<String>, AmError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(fields_error(&format!("metadata.{key}"), "string")),
    }
}

fn json_strings(value: Option<&Value>, field: &str) -> Result<Vec<String>, AmError> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| fields_error(field, "list of strings"))
            })
            .collect(),
        Some(_) => Err(fields_error(field, "list of strings")),
    }
}

fn metadata_from_json(map: &JsonMap<String, Value>) -> Result<Metadata, AmError> {
    let id = match map.get("id") {
        Some(Value::String(id)) => id.clone(),
        _ => return Err(fields_error("metadata.id", "string")),
    };
    let text =
        |key: &str| -> Result<String, AmError> { Ok(json_string(map, key)?.unwrap_or_default()) };
    let mut metadata = Metadata::with_id(&id);
    metadata.name = text("name")?;
    metadata.version = text("version")?;
    metadata.owner = text("owner")?;
    metadata.loading = json_string(map, "loading")?;
    metadata.description = text("description")?;
    metadata.category = text("category")?;
    metadata.class = text("class")?;
    metadata.audience = match map.get("audience") {
        None | Some(Value::Null) => Audience::default(),
        Some(Value::Object(a)) => Audience {
            domains: json_strings(a.get("domains"), "metadata.audience.domains")?,
            roles: json_strings(a.get("roles"), "metadata.audience.roles")?,
            languages: json_strings(a.get("languages"), "metadata.audience.languages")?,
        },
        Some(_) => return Err(fields_error("metadata.audience", "object")),
    };
    metadata.use_cases = match map.get("useCases") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                let case = item
                    .as_object()
                    .ok_or_else(|| fields_error("metadata.useCases", "list of use cases"))?;
                let part = |key: &str| match case.get(key) {
                    None | Some(Value::Null) => Ok(String::new()),
                    Some(Value::String(s)) => Ok(s.clone()),
                    Some(_) => Err(fields_error("metadata.useCases", "list of use cases")),
                };
                Ok(UseCase {
                    title: part("title")?,
                    summary: part("summary")?,
                })
            })
            .collect::<Result<_, AmError>>()?,
        Some(_) => return Err(fields_error("metadata.useCases", "list of use cases")),
    };
    metadata.status = json_string(map, "status")?;
    metadata.supersedes = json_string(map, "supersedes")?;
    for (key, value) in map {
        if !TYPED_METADATA_KEYS.contains(&key.as_str()) {
            metadata
                .extra
                .insert(key.clone(), yaml_of(value, &format!("metadata.{key}"))?);
        }
    }
    Ok(metadata)
}

/// The authored kind written for a kind: its first table spelling, or its kebab-case name.
fn authored_kind(kind: PartKind) -> String {
    kind_info(kind)
        .authored_spellings
        .first()
        .map(|s| (*s).to_owned())
        .unwrap_or_else(|| kebab_kind_name(kind))
}

/// Step 14's tail: place, render, digest.
pub(crate) fn finish(
    kind: PartKind,
    manifest: Manifest,
    text: Option<&str>,
) -> Result<ModelPart, AmError> {
    let path = part_file_path(kind, &manifest.metadata.id)?;
    let file = render_part(kind, &manifest, text)?;
    let base = path.rsplit('/').next().unwrap_or(&path).to_owned();
    let files = PartFiles(BTreeMap::from([(base, file.content)]));
    Ok(ModelPart {
        kind,
        content_digest: content_digest(&files),
        manifest,
        manifest_path: path,
        files,
    })
}

fn is_markdown(kind: PartKind) -> bool {
    kind_info(kind)
        .file_convention
        .is_some_and(FileConvention::is_markdown)
}

// ---- form (step 15) ----

fn string_schema() -> Value {
    json!({ "type": "string" })
}

fn list_schema(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

fn object_schema() -> Value {
    json!({ "type": "object" })
}

/// The body keys the kind's typed readers read, each with its schema.
fn spec_properties(kind: PartKind) -> Vec<(&'static str, Value)> {
    let strings = || list_schema(string_schema());
    match kind {
        PartKind::WorkspaceProfile => vec![
            ("requiredPracticePacks", list_schema(object_schema())),
            (
                "selectablePracticePacks",
                list_schema(json!({ "type": ["string", "object"] })),
            ),
            (
                "fixedPracticePacks",
                list_schema(json!({ "type": ["string", "object"] })),
            ),
            ("governanceMetadata", object_schema()),
            ("requiredBrandOverlays", object_schema()),
            ("requiredJurisdictionOverlays", object_schema()),
            ("requiredLanguageOverlays", object_schema()),
            ("requiredStyleOverlays", object_schema()),
            ("requiredSourceWorkflowOverlays", object_schema()),
            ("requiredHandoffOverlays", object_schema()),
            ("requiredControlLoop", object_schema()),
            ("allowedOperatingModes", strings()),
            ("defaultMode", string_schema()),
            ("toolPermissions", object_schema()),
        ],
        PartKind::ControlLoop => vec![
            ("strategy", string_schema()),
            ("trigger", string_schema()),
            ("requires", strings()),
            ("modes", strings()),
        ],
        PartKind::ToolBinding => vec![("bindings", list_schema(object_schema()))],
        PartKind::Capability => vec![("defaultToolPolicy", object_schema())],
        PartKind::PracticePack => vec![
            ("toolchain", object_schema()),
            ("appliesTo", object_schema()),
        ],
        PartKind::Memory => vec![
            ("retains", strings()),
            ("excludes", strings()),
            ("retention", object_schema()),
            ("store", object_schema()),
        ],
        PartKind::Command => vec![(
            "argument-hint",
            json!({ "type": "string", "maxLength": 200 }),
        )],
        PartKind::OutputContract => vec![("sections", list_schema(object_schema()))],
        PartKind::Vocabulary => vec![
            ("axis", string_schema()),
            ("terms", list_schema(object_schema())),
        ],
        PartKind::Journey => vec![
            ("seams", strings()),
            ("trigger", string_schema()),
            ("stages", list_schema(object_schema())),
        ],
        PartKind::BrandOverlay
        | PartKind::JurisdictionOverlay
        | PartKind::LanguageOverlay
        | PartKind::SourceWorkflowOverlay
        | PartKind::StyleOverlay
        | PartKind::HandoffOverlay => vec![("appliesTo", object_schema())],
        _ => Vec::new(),
    }
}

// SPEC-GAP-14: step 15 names the typed reader keys per kind by example only (WorkspaceProfile, Journey,
// ToolBinding); the other kinds' properties follow the readers of step 7, and `appliesTo` is placed on
// PracticePack and the overlay kinds, the kinds that pin workspaces.
fn form_schema(kind: PartKind) -> Value {
    let metadata = json!({
        "type": "object",
        "required": ["id"],
        "properties": {
            "id": { "type": "string", "pattern": r"^(@[a-z][a-z0-9-]*(/[A-Za-z0-9][A-Za-z0-9-]*)+|urn:ordin:[a-z][a-z0-9-]*(:[A-Za-z0-9][A-Za-z0-9._-]*)+)$" },
            "name": { "type": "string", "minLength": 1, "maxLength": 200 },
            "version": { "type": "string", "pattern": r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$" },
            "owner": { "type": "string" },
            "loading": { "type": "string" },
            "description": { "type": "string", "maxLength": 2000 },
            "category": { "type": "string" },
            "class": { "type": "string" },
            "audience": {
                "type": "object",
                "properties": {
                    "domains": list_schema(string_schema()),
                    "roles": list_schema(string_schema()),
                    "languages": list_schema(string_schema()),
                },
            },
            "useCases": list_schema(json!({
                "type": "object",
                "properties": { "title": string_schema(), "summary": string_schema() },
            })),
            "status": { "type": "string", "enum": ["draft", "approved", "deprecated"] },
            "supersedes": { "type": "string" },
        },
        "additionalProperties": true,
    });
    let spec_props: JsonMap<String, Value> = spec_properties(kind)
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    let mut properties = JsonMap::new();
    properties.insert("metadata".into(), metadata);
    properties.insert(
        "spec".into(),
        json!({ "type": "object", "properties": spec_props, "additionalProperties": true }),
    );
    if is_markdown(kind) {
        properties.insert("text".into(), string_schema());
    }
    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": kebab_kind_name(kind),
        "type": "object",
        "required": ["metadata"],
        "properties": properties,
    });
    if kind_info(kind).file_convention.is_none() {
        schema["readOnly"] = Value::Bool(true);
    }
    schema
}

// ---- write-up (step 18) ----

fn write_up(draft: &AgentDraft) -> String {
    let mut sections: Vec<(&str, String)> = Vec::new();
    let bullets = |items: &mut dyn Iterator<Item = String>| -> String {
        items.map(|item| format!("- {item}\n")).collect()
    };
    if !draft.purpose.trim().is_empty() {
        sections.push(("role-identity", format!("{}\n", draft.purpose.trim_end())));
    }
    let jobs = bullets(&mut draft.jobs.iter().map(|job| job.text.clone()));
    if !jobs.is_empty() {
        sections.push(("responsibilities", jobs));
    }
    let tasks = bullets(&mut draft.tasks.iter().cloned());
    if !tasks.is_empty() {
        sections.push(("outputs-artefacts", tasks));
    }
    let decisions = bullets(
        &mut draft
            .will
            .iter()
            .map(|w| format!("Will: {w}"))
            .chain(draft.wont.iter().map(|w| format!("Won't: {w}"))),
    );
    if !decisions.is_empty() {
        sections.push(("decisions-owned", decisions));
    }
    let needs = bullets(&mut draft.needs.iter().cloned());
    if !needs.is_empty() {
        sections.push(("tools-systems", needs));
    }
    let mut out = format!("# {}\n", draft.name.trim());
    for (heading, content) in sections {
        out.push_str(&format!("\n## {heading}\n\n{content}"));
    }
    out
}

// ---- agent parts (step 17) ----

/// The relation key a WorkspaceProfile lists a part of `kind` under, and whether it holds one value.
fn composition_key(kind: PartKind) -> Option<(&'static str, bool)> {
    Some(match kind {
        PartKind::Rule => ("defaultRules", false),
        PartKind::Skill => ("skills", false),
        PartKind::Command => ("commands", false),
        PartKind::Capability => ("capabilities", false),
        PartKind::ContextResource => ("contextResources", false),
        PartKind::ToolBinding => ("toolBindings", false),
        PartKind::OutputContract => ("outputContracts", false),
        PartKind::Memory => ("memory", true),
        PartKind::ControlLoop => ("controlLoop", true),
        PartKind::PracticePack => ("fixedPracticePacks", false),
        PartKind::OperatingMode => ("allowedOperatingModes", false),
        _ => return None,
    })
}

/// The generated part's fields with the identifier, name, version and status filled when absent.
fn generated_fields(part: &GeneratedPart, publisher: &str) -> Result<Value, AmError> {
    let mut fields = match &part.fields {
        Value::Null => JsonMap::new(),
        Value::Object(map) => map.clone(),
        _ => return Err(fields_error("fields", "object")),
    };
    let metadata = fields
        .entry("metadata")
        .or_insert_with(|| Value::Object(JsonMap::new()));
    let Value::Object(metadata) = metadata else {
        return Err(fields_error("metadata", "object"));
    };
    if !metadata.contains_key("id") {
        let id = mint_identifier(part.kind, publisher, None, &part.name)?;
        metadata.insert("id".into(), Value::String(id));
    }
    for (key, default) in [
        ("name", part.name.as_str()),
        ("version", "1.0.0"),
        ("status", "draft"),
    ] {
        if !metadata.contains_key(key) {
            metadata.insert(key.into(), Value::String(default.to_owned()));
        }
    }
    Ok(Value::Object(fields))
}

fn draft_metadata(id: &str, name: &str, description: &str) -> Metadata {
    let mut metadata = Metadata::with_id(id);
    metadata.name = name.to_owned();
    metadata.version = "1.0.0".to_owned();
    metadata.description = description.to_owned();
    metadata.status = Some("draft".to_owned());
    metadata
}

impl DraftModel for FieldModel {
    fn agent_parts_from_fields(
        &self,
        draft: &AgentDraft,
        publisher: &str,
        reused: &[ReusedPart],
        generated: &[GeneratedPart],
    ) -> Result<Vec<ModelPart>, AmError> {
        // Step 17.1.
        let name = draft.name.trim();
        if name.is_empty() {
            return Err(fields_error("name", "non-empty text"));
        }
        if !matches(&PUBLISHER, publisher) {
            return Err(AmError::IdInvalid {
                id: publisher.to_owned(),
            });
        }
        // Step 17.2.
        let workspace_id = mint_identifier(PartKind::WorkspaceProfile, publisher, None, name)?;
        let slug = Urn::parse(&workspace_id)
            .map(|urn| urn.slug)
            .ok_or_else(|| AmError::IdInvalid {
                id: workspace_id.clone(),
            })?;
        let role_id = format!(
            "{}{publisher}:workspaces:{slug}:system-role",
            crate::ident::URN_PREFIX
        );
        // Step 17.3.
        let mut generated_parts = Vec::with_capacity(generated.len());
        for part in generated {
            let composable = composition_key(part.kind).is_some()
                && kind_info(part.kind).file_convention.is_some();
            if !composable {
                return Err(AmError::DraftKindNotComposable { kind: part.kind });
            }
            let fields = generated_fields(part, publisher)?;
            generated_parts.push(self.part_from_fields(part.kind, &fields)?);
        }
        // Step 17.4.
        let role = finish(
            PartKind::SystemRole,
            Manifest {
                api_version: API_VERSION.to_owned(),
                authored_kind: authored_kind(PartKind::SystemRole),
                metadata: draft_metadata(&role_id, &format!("{name} role"), &draft.purpose),
                body: BTreeMap::new(),
            },
            Some(&write_up(draft)),
        )?;
        // Step 17.5: reused first, then generated, in input order.
        let mut body: BTreeMap<String, YamlValue> = BTreeMap::new();
        body.insert("systemRole".into(), YamlValue::Str(role_id));
        let members = reused.iter().map(|r| (r.kind, r.identifier.clone())).chain(
            generated_parts
                .iter()
                .map(|p| (p.kind, p.manifest.metadata.id.clone())),
        );
        let mut singles_seen: Vec<PartKind> = Vec::new();
        for (kind, id) in members {
            let (key, single) =
                composition_key(kind).ok_or(AmError::DraftKindNotComposable { kind })?;
            if single {
                if singles_seen.contains(&kind) {
                    return Err(AmError::DraftKindNotComposable { kind });
                }
                singles_seen.push(kind);
                body.insert(key.into(), YamlValue::Str(id));
                continue;
            }
            let entry = body
                .entry(key.to_owned())
                .or_insert_with(|| YamlValue::Seq(Vec::new()));
            if let YamlValue::Seq(items) = entry {
                let value = YamlValue::Str(id);
                if !items.contains(&value) {
                    items.push(value);
                }
            }
        }
        let workspace = finish(
            PartKind::WorkspaceProfile,
            Manifest {
                api_version: API_VERSION.to_owned(),
                authored_kind: authored_kind(PartKind::WorkspaceProfile),
                metadata: draft_metadata(&workspace_id, name, &draft.purpose),
                body,
            },
            None,
        )?;
        // Step 17.6.
        let mut parts = vec![workspace, role];
        parts.extend(generated_parts);
        Ok(parts)
    }

    fn part_from_fields(&self, kind: PartKind, fields: &Value) -> Result<ModelPart, AmError> {
        let fields = fields
            .as_object()
            .ok_or_else(|| fields_error("fields", "object"))?;
        let metadata = match fields.get("metadata") {
            Some(Value::Object(map)) => metadata_from_json(map)?,
            _ => return Err(fields_error("metadata.id", "string")),
        };
        let body = match fields.get("spec") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(map)) => map
                .iter()
                .map(|(key, value)| Ok((key.clone(), yaml_of(value, &format!("spec.{key}"))?)))
                .collect::<Result<_, AmError>>()?,
            Some(_) => return Err(fields_error("spec", "object")),
        };
        let text = match fields.get("text") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.as_str()),
            Some(_) => return Err(fields_error("text", "string")),
        };
        let manifest = Manifest {
            api_version: API_VERSION.to_owned(),
            authored_kind: authored_kind(kind),
            metadata,
            body,
        };
        finish(kind, manifest, text)
    }

    fn fields_of(&self, part: &ModelPart) -> Value {
        let spec: JsonMap<String, Value> = part
            .manifest
            .body
            .iter()
            .map(|(k, v)| (k.clone(), json_of(v)))
            .collect();
        let text = if is_markdown(part.kind) {
            let base = part
                .manifest_path
                .rsplit('/')
                .next()
                .unwrap_or(&part.manifest_path);
            part.files
                .0
                .get(base)
                .and_then(|bytes| markdown_body(&part.manifest_path, bytes))
                .map(Value::String)
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        json!({
            "metadata": metadata_json(&part.manifest.metadata),
            "spec": spec,
            "text": text,
        })
    }

    fn form(&self, kind: PartKind) -> Value {
        form_schema(kind)
    }

    fn scaffold(&self, kind: PartKind, name: &str, identifier: &str) -> Result<ModelPart, AmError> {
        if !is_part_identifier(identifier) {
            return Err(AmError::IdInvalid {
                id: identifier.to_owned(),
            });
        }
        let mut metadata = Metadata::with_id(identifier);
        metadata.name = name.to_owned();
        metadata.version = "1.0.0".to_owned();
        metadata.status = Some("draft".to_owned());
        let manifest = Manifest {
            api_version: API_VERSION.to_owned(),
            authored_kind: authored_kind(kind),
            metadata,
            body: BTreeMap::new(),
        };
        let text = is_markdown(kind).then(|| format!("# {name}\n"));
        finish(kind, manifest, text.as_deref())
    }

    fn render_write_up(&self, draft: &AgentDraft) -> String {
        write_up(draft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_conversion() {
        assert_eq!(yaml_of(&json!(3), "x"), Ok(YamlValue::Int(3)));
        assert_eq!(
            yaml_of(&json!({"$decimal": "1.50"}), "x"),
            Ok(YamlValue::Decimal("1.50".into()))
        );
        for bad in [
            json!(1.5),
            json!(u64::MAX),
            json!({"$decimal": "abc"}),
            json!({"$decimal": 1}),
        ] {
            assert_eq!(
                yaml_of(&bad, "spec.x"),
                Err(AmError::DraftFieldsInvalid {
                    field: "spec.x".into(),
                    expected: NUMBER_EXPECTED
                })
            );
        }
        assert_eq!(
            json_of(&YamlValue::Decimal("2.0".into())),
            json!({"$decimal": "2.0"})
        );
    }

    #[test]
    fn field_errors() {
        let model = FieldModel;
        let cases = [
            (json!([]), "fields"),
            (json!({}), "metadata.id"),
            (json!({"metadata": {"id": 3}}), "metadata.id"),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "name": 3}}),
                "metadata.name",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "audience": 3}}),
                "metadata.audience",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "audience": {"roles": [1]}}}),
                "metadata.audience.roles",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "useCases": [3]}}),
                "metadata.useCases",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "useCases": 3}}),
                "metadata.useCases",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "useCases": [{"title": 1}]}}),
                "metadata.useCases",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r"}, "spec": []}),
                "spec",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r"}, "text": 3}),
                "text",
            ),
            (
                json!({"metadata": {"id": "urn:ordin:a:rules:r", "x": 1.5}}),
                "metadata.x",
            ),
        ];
        for (fields, field) in cases {
            match model.part_from_fields(PartKind::Rule, &fields) {
                Err(AmError::DraftFieldsInvalid { field: got, .. }) => assert_eq!(got, field),
                other => panic!("{fields}: {other:?}"),
            }
        }
    }

    #[test]
    fn scaffold_and_form() {
        let model = FieldModel;
        let part = model
            .scaffold(PartKind::Rule, "Tone", "urn:ordin:a:rules:tone")
            .unwrap();
        assert_eq!(part.manifest_path, "models/@a/rules/tone.md");
        assert_eq!(
            String::from_utf8(part.files.0["tone.md"].clone()).unwrap(),
            "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:a:rules:tone\n  name: Tone\n  version: 1.0.0\n  status: draft\n---\n# Tone\n"
        );
        let yaml = model
            .scaffold(PartKind::Capability, "C", "urn:ordin:a:capabilities:c")
            .unwrap();
        assert_eq!(model.fields_of(&yaml)["text"], Value::Null);
        assert_eq!(
            model
                .scaffold(PartKind::Rule, "x", "not an id")
                .unwrap_err()
                .code(),
            "AM_ID_INVALID"
        );
        assert_eq!(
            model
                .scaffold(PartKind::CrewProfile, "x", "urn:ordin:a:crews:x")
                .unwrap_err()
                .code(),
            "AM_NO_FILE_CONVENTION"
        );
        assert_eq!(
            model.form(PartKind::CrewProfile)["readOnly"],
            Value::Bool(true)
        );
        assert!(model.form(PartKind::Rule).get("readOnly").is_none());
        assert!(model.form(PartKind::Rule)["properties"]
            .get("text")
            .is_some());
        assert!(
            model.form(PartKind::Journey)["properties"]["spec"]["properties"]
                .get("stages")
                .is_some()
        );
        assert_eq!(authored_kind(PartKind::BrandOverlay), "brand-overlay");
    }
}
