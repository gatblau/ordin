//! Table-driven parity over the committed fixtures in `testdata/parity/agent-model/` (Playbook Step 8,
//! as amended by the user's decision on committed fixtures; testdata/README.md).
//!
//! - `tree/`: a parts tree covering every part kind and every file format AgentModel.md names. It is
//!   read twice, the second time with the vocabulary set built from the first, and every part is
//!   compared with `expected/tree.json`.
//! - `refusals/`: manifests each refused for one reason, compared with `expected/refusals.json`.
//! - `formats/`: `withdrawn.yaml`, `ordin.lock` and `bundle.manifest.json` with their expected values.
//!
//! A missing or empty fixture folder fails the test; it never skips.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use ordin_agent_model::*;

fn fixture_root() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = crate_dir
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root");
    workspace
        .join("testdata")
        .join("parity")
        .join("agent-model")
}

/// Every file under `dir`, keyed by its '/'-separated path relative to `dir`.
fn read_dir(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("fixture folder {} cannot be read: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .expect("under the base")
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.insert(
                    relative,
                    std::fs::read(&path).expect("a readable fixture file"),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn fixtures(name: &str) -> BTreeMap<String, Vec<u8>> {
    let root = fixture_root();
    assert!(
        root.is_dir(),
        "the parity fixture folder {} is missing",
        root.display()
    );
    let files = read_dir(&root.join(name));
    assert!(
        !files.is_empty(),
        "the parity fixture folder {}/{name} is empty",
        root.display()
    );
    files
}

fn expected(name: &str) -> Value {
    let path = fixture_root().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} cannot be read: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

fn code(error: &AmError) -> Value {
    json!({ "error": error.code() })
}

fn or_code<T>(result: Result<T, AmError>, describe: impl FnOnce(T) -> Value) -> Value {
    match result {
        Ok(value) => describe(value),
        Err(error) => code(&error),
    }
}

fn kind_json(kind: PartKind) -> Value {
    serde_json::to_value(kind).expect("a kind serialises")
}

/// Every typed reader whose result is not empty, by reader name.
fn readers(manifest: &Manifest) -> Value {
    let mut out = serde_json::Map::new();
    let mut put = |name: &str, value: Value| {
        let empty = matches!(&value, Value::Null | Value::Bool(false))
            || value.as_array().is_some_and(Vec::is_empty);
        if !empty {
            out.insert(name.to_owned(), value);
        }
    };
    put(
        "requiredPracticePacks",
        or_code(manifest.required_practice_packs(), |rules| {
            serde_json::to_value(rules).expect("rules serialise")
        }),
    );
    put(
        "selectablePracticePacks",
        or_code(manifest.selectable_practice_packs(), |packs| {
            json!(packs
                .iter()
                .map(|p| json!({ "id": p.id, "description": p.description }))
                .collect::<Vec<_>>())
        }),
    );
    put(
        "fixedPracticePacks",
        or_code(manifest.fixed_practice_packs(), |entries| {
            json!(entries
                .iter()
                .map(|e| match e {
                    FixedPackEntry::Pack(id) => json!({ "pack": id }),
                    FixedPackEntry::Ask { class, ids } =>
                        json!({ "ask": { "class": class, "ids": ids } }),
                })
                .collect::<Vec<_>>())
        }),
    );
    let mut offers = serde_json::Map::new();
    for axis in overlay_order() {
        match manifest.overlay_offer(*axis) {
            Ok(None) => {}
            Ok(Some(offer)) => {
                offers.insert(
                    kind_json(*axis).as_str().unwrap_or_default().to_owned(),
                    json!({ "available": offer.available, "default": offer.default }),
                );
            }
            Err(error) => {
                offers.insert(
                    kind_json(*axis).as_str().unwrap_or_default().to_owned(),
                    code(&error),
                );
            }
        }
    }
    if !offers.is_empty() {
        put("overlayOffers", Value::Object(offers));
    }
    put(
        "requiredOverlays",
        or_code(manifest.required_overlays(), |pairs| {
            json!(pairs
                .iter()
                .map(|(kind, ids)| json!([kind_json(*kind), ids]))
                .collect::<Vec<_>>())
        }),
    );
    put(
        "requiredControlLoop",
        or_code(manifest.required_control_loop(), |rule| match rule {
            None => Value::Null,
            Some(rule) => json!({ "required": rule.required, "ids": rule.ids }),
        }),
    );
    put(
        "allowedOperatingModes",
        json!(manifest.allowed_operating_modes()),
    );
    put("defaultMode", json!(manifest.default_mode()));
    put(
        "controlLoop",
        or_code(manifest.control_loop(), |spec| json!(spec)),
    );
    put(
        "prohibitedVerbs",
        or_code(manifest.prohibited_verbs(), |v| json!(v)),
    );
    put(
        "toolBindings",
        or_code(manifest.tool_bindings(), |v| json!(v)),
    );
    put(
        "requiresHumanApproval",
        or_code(manifest.requires_human_approval(), |v| json!(v)),
    );
    put(
        "packToolchain",
        or_code(manifest.pack_toolchain(), |toolchain| match toolchain {
            None => Value::Null,
            Some(t) => {
                json!({ "forbiddenFlags": t.forbidden_flags, "coverageFloor": t.coverage_floor })
            }
        }),
    );
    put(
        "memoryContract",
        or_code(manifest.memory_contract(), |m| json!(m)),
    );
    put(
        "argumentHint",
        or_code(manifest.argument_hint(), |h| json!(h)),
    );
    put("publishGate", json!(manifest.publish_gate()));
    put(
        "appliesTo",
        or_code(manifest.applies_to(), |pins| {
            json!(pins
                .iter()
                .map(|p| json!({ "id": p.id, "range": p.range }))
                .collect::<Vec<_>>())
        }),
    );
    put(
        "vocabulary",
        or_code(manifest.vocabulary(), |body| match body {
            None => Value::Null,
            Some(body) => json!({
                "axis": body.axis,
                "terms": body.terms.iter().map(|t| json!({
                    "code": t.code, "label": t.label, "description": t.description
                })).collect::<Vec<_>>(),
            }),
        }),
    );
    put(
        "journey",
        or_code(manifest.journey(), |body| match body {
            None => Value::Null,
            Some(body) => json!({
                "seams": body.seams,
                "trigger": body.trigger,
                "stages": body.stages.iter().map(|s| json!({
                    "id": s.id, "name": s.name, "role": s.role, "agent": s.agent,
                    "produces": s.produces, "handoverPlaceType": s.handover_place_type,
                    "handoverPlaceTarget": s.handover_place_target,
                    "answerWithinDays": s.answer_within_days,
                    "next": s.next.as_ref().map(|n| n.iter().map(|e| json!({
                        "stage": e.stage, "when": e.when
                    })).collect::<Vec<_>>()),
                    "together": s.together,
                    "sendsBackTo": s.sends_back_to.iter().map(|b| json!({
                        "stage": b.stage, "when": b.when, "maxRounds": b.max_rounds
                    })).collect::<Vec<_>>(),
                    "slackChannel": s.slack_channel.as_ref().map(|c| json!({
                        "id": c.id, "name": c.name
                    })),
                })).collect::<Vec<_>>(),
            }),
        }),
    );
    put(
        "outputContract",
        or_code(OutputContract::read(manifest), |contract| {
            json!(contract
                .sections()
                .iter()
                .map(|s| json!({
                    "name": s.name, "meaning": s.meaning, "required": s.required,
                    "evidenceThreshold": s.evidence_threshold, "derived": s.derived
                }))
                .collect::<Vec<_>>())
        }),
    );
    Value::Object(out)
}

/// The manifest file of a part, by its base name.
fn manifest_bytes(part: &ModelPart) -> &[u8] {
    let base = part.manifest_path.rsplit('/').next().unwrap_or_default();
    &part.files.0[base]
}

fn describe(part: &ModelPart) -> Value {
    let manifest = &part.manifest;
    let body = markdown_body(&part.manifest_path, manifest_bytes(part));
    let renders_unchanged = render_part(part.kind, manifest, body.as_deref())
        .map(|file| file.content == manifest_bytes(part))
        .unwrap_or(false);
    json!({
        "manifestPath": part.manifest_path,
        "kind": kind_json(part.kind),
        "authoredKind": manifest.authored_kind,
        "id": manifest.metadata.id,
        "name": manifest.metadata.name,
        "version": manifest.metadata.version,
        "status": or_code(manifest.status_or_default(), |s| json!(s)),
        "slug": manifest.slug(),
        "files": part.files.0.keys().collect::<Vec<_>>(),
        "contentDigest": part.content_digest.0,
        "rendersUnchanged": renders_unchanged,
        "placedAt": or_code(part_file_path(part.kind, &manifest.metadata.id), |p| json!(p)),
        "references": manifest.references().iter().map(|r| json!([r.relation, r.id])).collect::<Vec<_>>(),
        "readers": readers(manifest),
    })
}

fn describe_reading(reading: &TreeReading) -> Value {
    json!({
        "parts": reading.parts.iter().map(describe).collect::<Vec<_>>(),
        "refused": reading.refused.iter().map(|(p, e)| json!({ "path": p, "code": e.code() })).collect::<Vec<_>>(),
        "unownedFiles": reading.unowned_files,
    })
}

/// Compares part by part, so a failure names the part, and prints the whole actual value.
fn assert_reading(actual: &Value, expected: &Value, name: &str) {
    let pretty = serde_json::to_string_pretty(actual).unwrap_or_default();
    for key in ["refused", "unownedFiles"] {
        assert_eq!(
            actual[key], expected[key],
            "{name}: {key} differs; actual reading:\n{pretty}"
        );
    }
    let actual_parts = actual["parts"].as_array().cloned().unwrap_or_default();
    let expected_parts = expected["parts"].as_array().cloned().unwrap_or_default();
    for (a, e) in actual_parts.iter().zip(expected_parts.iter()) {
        assert_eq!(
            a, e,
            "{name}: part {} differs; actual reading:\n{pretty}",
            e["manifestPath"]
        );
    }
    assert_eq!(
        actual_parts.len(),
        expected_parts.len(),
        "{name}: part count differs; actual reading:\n{pretty}"
    );
}

/// The two-pass read the spec prescribes: an empty vocabulary, then the set built from the first pass.
fn read_two_passes(files: &BTreeMap<String, Vec<u8>>) -> (TreeReading, TreeReading) {
    let first = read_tree(files, &VocabularySet::default());
    let vocabularies: Vec<&ModelPart> = first
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Vocabulary)
        .collect();
    let set = VocabularySet::from_parts(&vocabularies).expect("one vocabulary part per axis");
    let second = read_tree(files, &set);
    (first, second)
}

#[test]
fn parts_tree_reads_as_expected() {
    let files = fixtures("tree");
    let (first, second) = read_two_passes(&files);
    // The rule sub-variant needs the vocabulary: refused in the first pass only.
    assert_eq!(
        first
            .refused
            .iter()
            .map(|(p, e)| (p.as_str(), e.code()))
            .collect::<Vec<_>>(),
        vec![(
            "models/@default/rules/layer/output-language.md",
            "AM_KIND_UNKNOWN"
        )]
    );
    assert_reading(
        &describe_reading(&second),
        &expected("expected/tree.json"),
        "tree",
    );

    // Coverage: every kind a manifest can author, and every file format the spec names.
    let kinds: Vec<PartKind> = second.parts.iter().map(|p| p.kind).collect();
    for kind in all_kinds() {
        let authorable = !kind_info(*kind).authored_spellings.is_empty();
        assert_eq!(kinds.contains(kind), authorable, "{kind:?} coverage");
    }
    let conventions: Vec<FileConvention> = second
        .parts
        .iter()
        .filter(|p| {
            part_file_path(p.kind, &p.manifest.metadata.id)
                .ok()
                .as_deref()
                == Some(p.manifest_path.as_str())
        })
        .filter_map(|p| kind_info(p.kind).file_convention)
        .collect();
    for convention in [
        FileConvention::MarkdownFile,
        FileConvention::SkillDirectory,
        FileConvention::YamlFile,
        FileConvention::YamlInDirectory {
            file_name: "workspace-profile.yaml",
        },
        FileConvention::YamlInDirectory {
            file_name: "practice-pack.yaml",
        },
        FileConvention::ControlLoopFile,
    ] {
        assert!(conventions.contains(&convention), "{convention:?} coverage");
    }
    for extension in [".md", ".markdown", ".yaml", ".yml"] {
        assert!(
            second
                .parts
                .iter()
                .any(|p| p.manifest_path.ends_with(extension)),
            "{extension} coverage"
        );
    }
}

#[test]
fn every_tree_part_round_trips() {
    let files = fixtures("tree");
    let (_, reading) = read_two_passes(&files);
    let model = FieldModel;
    for part in &reading.parts {
        if kind_info(part.kind).file_convention.is_none() {
            continue;
        }
        let path = &part.manifest_path;
        // Render and read back: the same manifest (lossless model).
        // A legacy identifier has no placement: it is read, never rendered.
        if Urn::parse(&part.manifest.metadata.id).is_none() {
            continue;
        }
        let body = markdown_body(path, manifest_bytes(part));
        let rendered = render_part(part.kind, &part.manifest, body.as_deref())
            .unwrap_or_else(|e| panic!("{path}: {e:?}"));
        let back = parse_manifest(&rendered.path, &rendered.content)
            .expect("a rendered file parses")
            .expect("a rendered file is a manifest");
        assert_eq!(back, part.manifest, "{path}: render and read back");
        // Fields and back: the same bytes. `part_from_fields` writes the kind's first authored
        // spelling (step 14), so a part authored with another spelling (`ci-gate`, `rule-layer`)
        // comes back with that one line changed and nothing else.
        let mut respelt = part.manifest.clone();
        respelt.authored_kind = kind_info(part.kind).authored_spellings[0].to_owned();
        let expected_bytes = render_part(part.kind, &respelt, body.as_deref())
            .unwrap_or_else(|e| panic!("{path}: {e:?}"))
            .content;
        let again = model
            .part_from_fields(part.kind, &model.fields_of(part))
            .unwrap_or_else(|e| panic!("{path}: {e:?}"));
        let base = rendered.path.rsplit('/').next().unwrap_or_default();
        assert_eq!(
            again.files.0[base], expected_bytes,
            "{path}: fields round trip"
        );
        if respelt.authored_kind == part.manifest.authored_kind {
            assert_eq!(
                again.files.0[base], rendered.content,
                "{path}: fields round trip"
            );
        }
        // The fields satisfy the kind's form.
        let fields = model.fields_of(part);
        let form = model.form(part.kind);
        if let Err(failure) = validate(&fields, &form, "") {
            panic!(
                "{path}: fields_of does not satisfy form({:?}): {failure}",
                part.kind
            );
        }
    }
}

/// The subset of JSON Schema `form` uses: type, required, properties, items, enum, pattern, minLength,
/// maxLength.
fn validate(value: &Value, schema: &Value, at: &str) -> Result<(), String> {
    if let Some(types) = schema.get("type") {
        let names: Vec<&str> = match types {
            Value::String(t) => vec![t.as_str()],
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        let ok = names.iter().any(|t| match *t {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            _ => false,
        });
        if !ok {
            return Err(format!("{at}: not {names:?}"));
        }
    }
    if let (Some(required), Some(object)) = (schema.get("required"), value.as_object()) {
        for key in required
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !object.contains_key(key) {
                return Err(format!("{at}: {key} missing"));
            }
        }
    }
    if let (Some(properties), Some(object)) = (schema.get("properties"), value.as_object()) {
        for (key, item) in object {
            if let Some(sub) = properties.get(key) {
                validate(item, sub, &format!("{at}.{key}"))?;
            }
        }
    }
    if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
        for (i, item) in array.iter().enumerate() {
            validate(item, items, &format!("{at}[{i}]"))?;
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.iter().any(|a| a == value) {
                return Err(format!("{at}: {text} not allowed"));
            }
        }
        let chars = text.chars().count() as u64;
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| chars < m)
            || schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|m| chars > m)
        {
            return Err(format!("{at}: length {chars}"));
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            let re = regex::Regex::new(pattern).map_err(|e| format!("{at}: pattern {e}"))?;
            if !re.is_match(text) {
                return Err(format!("{at}: {text} does not match"));
            }
        }
    }
    Ok(())
}

#[test]
fn refusals_read_as_expected() {
    let files = fixtures("refusals");
    let reading = read_tree(&files, &VocabularySet::default());
    assert_reading(
        &describe_reading(&reading),
        &expected("expected/refusals.json"),
        "refusals",
    );
}

fn withdrawn_json(list: &WithdrawnListFile) -> Value {
    json!({
        "listSchema": list.list_schema,
        "entries": list.entries.iter().map(|e| {
            let mut part = json!({ "kind": kind_json(e.part.kind), "id": e.part.id });
            if let Some(v) = &e.part.version { part["version"] = json!(v); }
            if let Some(h) = &e.part.content_hash { part["contentHash"] = json!(h); }
            let mut entry = json!({ "part": part, "reason": e.reason, "withdrawnAt": e.withdrawn_at });
            if let Some(r) = &e.replacement {
                let mut replacement = json!({ "id": r.id });
                if let Some(v) = &r.version { replacement["version"] = json!(v); }
                entry["replacement"] = replacement;
            }
            entry
        }).collect::<Vec<_>>(),
    })
}

#[test]
fn file_formats_read_as_expected() {
    let files = fixtures("formats");
    // withdrawn.yaml: the expected value, and the canonical rendering is the file itself.
    let list = parse_withdrawn_list(&files["withdrawn.yaml"]).expect("withdrawn.yaml parses");
    assert_eq!(withdrawn_json(&list), expected("formats/withdrawn.json"));
    assert_eq!(
        render_withdrawn_list(&list).as_bytes(),
        files["withdrawn.yaml"].as_slice()
    );
    // ordin.lock: the expected value, and the canonical YAML is the file itself.
    let lock = LockFile::from_yaml(&files["ordin.lock"]).expect("ordin.lock parses");
    assert_eq!(
        serde_json::to_value(&lock).expect("serialises"),
        expected("formats/ordin.lock.json")
    );
    assert_eq!(
        lock.to_canonical_yaml().as_bytes(),
        files["ordin.lock"].as_slice()
    );
    // bundle.manifest.json: read into the typed form and written back to the same value.
    let bundle: BundleManifest =
        serde_json::from_slice(&files["bundle.manifest.json"]).expect("bundle manifest reads");
    assert_eq!(bundle.bundle_schema, BUNDLE_SCHEMA);
    assert_eq!(
        serde_json::to_value(&bundle).expect("serialises"),
        expected("formats/bundle.manifest.json")
    );
}
