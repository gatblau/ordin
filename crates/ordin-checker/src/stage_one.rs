//! Section C: the part checks of one part (port of `StageOne`) and the lifecycle form checks that need
//! no other part (Checker.md *Internal Logic* 7, 8 and 26).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use ordin_agent_model::{
    canonical_kind, check_task_body, is_part_identifier, markdown_body, mint_identifier, AmError,
    JourneyStage, LoadingMode, Manifest, ModelPart, PartFiles, PartKind, Urn, VocabularyAxis,
    VocabularySet, YamlValue, API_VERSION,
};
use regex::Regex;

use crate::body::{as_map, as_seq, get, str_at, text};
use crate::finding::{display_name, finding, msg, part_failure};
use crate::model::{Finding, Fix, FixAction, Severity};

static SLUG: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9-]{0,127}$").ok());
static TRIGGER: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9-]{0,63}$").ok());
/// The Rules Engine's form of a prohibited verb (RulesEngine.md A.4; `ordin_rules` `verb_shape`).
static VERB: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9_-]+(\.[a-z0-9_-]+)?$").ok());
static SLACK_CHANNEL_ID: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[CG][A-Z0-9]{8,20}$").ok());

fn matches(re: &LazyLock<Option<Regex>>, s: &str) -> bool {
    re.as_ref().is_some_and(|re| re.is_match(s))
}

// `phases` (the loop's former step list, replaced by `modes`) is the one field treated as retired.
const RETIRED_LOOP_FIELDS: [&str; 1] = ["phases"];

/// The highest number of stages a lifecycle holds (2C §Lifecycle).
const MAX_STAGES: usize = 30;

/// What the checks of one part need: its kind, its manifest and its display name, and the Markdown
/// body when the part's files are at hand (a part read from a source carries none).
pub(crate) struct PartView<'a> {
    pub kind: PartKind,
    pub manifest: &'a Manifest,
    pub body: Option<String>,
}

/// The Markdown body of a part's manifest file, None for a YAML part or when the files are absent.
pub(crate) fn body_of_files(manifest_path: &str, files: &PartFiles) -> Option<String> {
    let (key, bytes) = files
        .0
        .iter()
        .filter(|(key, _)| manifest_path.ends_with(key.as_str()))
        .max_by_key(|(key, _)| key.len())?;
    markdown_body(key, bytes)
}

impl<'a> PartView<'a> {
    pub(crate) fn of(part: &'a ModelPart) -> PartView<'a> {
        PartView {
            kind: part.kind,
            manifest: &part.manifest,
            body: body_of_files(&part.manifest_path, &part.files),
        }
    }

    fn id(&self) -> &str {
        &self.manifest.metadata.id
    }

    fn name(&self) -> &str {
        display_name(&self.manifest.metadata.name, &self.manifest.metadata.id)
    }

    fn fail(&self, code: &str, fixes: Vec<Fix>, detail: String) -> Finding {
        part_failure(code, self.id(), self.name(), fixes, detail)
    }
}

fn change(field: &str) -> Vec<Fix> {
    vec![Fix::ChangeItYourself {
        field: field.to_owned(),
    }]
}

/// The publisher of an identifier, for the identifier fix: the URN's, the legacy `@<publisher>/…`
/// spelling's, else `default`.
fn publisher_of(id: &str) -> String {
    if let Some(urn) = Urn::parse(id) {
        return urn.publisher;
    }
    id.strip_prefix('@')
        .and_then(|rest| rest.split('/').next())
        .filter(|p| !p.is_empty())
        .unwrap_or("default")
        .to_owned()
}

/// The last segment of an identifier: the slug the grammar applies to.
fn slug_segment(id: &str) -> &str {
    id.rsplit([':', '/']).next().unwrap_or(id)
}

/// Step 26: a name turned into a valid identifier, else the field opened.
// spec: ordin-desktop/components/Checker#ADR-094
fn identifier_fix(view: &PartView<'_>) -> Vec<Fix> {
    let metadata = &view.manifest.metadata;
    match mint_identifier(view.kind, &publisher_of(&metadata.id), None, &metadata.name) {
        Ok(value) => vec![Fix::FixIt {
            action: FixAction::MintIdentifier {
                field: "metadata.id".to_owned(),
                value,
            },
        }],
        Err(_) => change("metadata.id"),
    }
}

fn is_loading_mode(text: &str) -> bool {
    serde_json::from_value::<LoadingMode>(serde_json::Value::String(text.to_owned())).is_ok()
}

/// The assay policy rule (port of `validateAssayPolicy`).
// Read from the body key `assayPolicy`, either a list of checks or a map with `checks`; each check is
// a map with `type` (`deterministic`, `narrative` or `scored`), `blocking`, `threshold`, `direction`.
fn assay_policy_problem(manifest: &Manifest) -> Option<String> {
    let value = get(&manifest.body, "assayPolicy")?;
    let checks = match value {
        YamlValue::Seq(items) => items.as_slice(),
        YamlValue::Map(map) => match get(map, "checks") {
            Some(YamlValue::Seq(items)) => items.as_slice(),
            None => &[],
            Some(_) => return Some("assayPolicy.checks is not a list".to_owned()),
        },
        _ => return Some("assayPolicy is neither a list nor a map".to_owned()),
    };
    for (index, check) in checks.iter().enumerate() {
        let Some(map) = as_map(check) else {
            return Some(format!("assayPolicy check {index} is not a map"));
        };
        let blocking = matches!(get(map, "blocking"), Some(YamlValue::Bool(true)));
        let kind = str_at(map, "type").unwrap_or_default();
        let problem = match kind.as_str() {
            "deterministic" if !blocking => Some("a deterministic check must block"),
            "narrative" if blocking => Some("a narrative check must not block"),
            "scored"
                if get(map, "threshold").is_none()
                    || str_at(map, "direction").is_none_or(|d| d.trim().is_empty()) =>
            {
                Some("a scored check needs a threshold and a direction")
            }
            "deterministic" | "narrative" | "scored" => None,
            _ => Some("the check type is not deterministic, narrative or scored"),
        };
        if let Some(problem) = problem {
            return Some(format!("assayPolicy check {index}: {problem}"));
        }
    }
    None
}

/// The stopping rules of step 7, in order: the first one that fails.
fn stopping_rule(view: &PartView<'_>, vocabulary: &VocabularySet) -> Option<Finding> {
    let manifest = view.manifest;
    let metadata = &manifest.metadata;
    if manifest.api_version != API_VERSION {
        return Some(view.fail(
            "apiversion_unknown",
            change("apiVersion"),
            format!("apiVersion {:?} is not {API_VERSION}", manifest.api_version),
        ));
    }
    if canonical_kind(&manifest.authored_kind, vocabulary).is_none() {
        return Some(view.fail(
            "kind_unknown",
            change("kind"),
            format!("kind {:?} is not a known kind", manifest.authored_kind),
        ));
    }
    for (field, value) in [
        ("metadata.id", &metadata.id),
        ("metadata.version", &metadata.version),
        ("metadata.owner", &metadata.owner),
    ] {
        if value.trim().is_empty() {
            return Some(view.fail(
                "metadata_required",
                vec![Fix::FixIt {
                    action: FixAction::AskMissingField {
                        field: field.to_owned(),
                    },
                }],
                format!("{field} is required"),
            ));
        }
    }
    if matches!(
        view.kind,
        PartKind::WorkspaceProfile | PartKind::PracticePack
    ) {
        if metadata.category.trim().is_empty() || metadata.audience.roles.is_empty() {
            return Some(
                view.fail(
                    "descriptive_metadata_missing",
                    change("metadata.category"),
                    "metadata.category and at least one metadata.audience.roles entry are required"
                        .to_owned(),
                ),
            );
        }
        let mut cited: Vec<(VocabularyAxis, &str, &str)> = vec![(
            VocabularyAxis::Category,
            "metadata.category",
            &metadata.category,
        )];
        cited.extend(metadata.audience.roles.iter().map(|r| {
            (
                VocabularyAxis::AudienceRole,
                "metadata.audience.roles",
                r.as_str(),
            )
        }));
        if !metadata.class.is_empty() {
            cited.push((VocabularyAxis::PackClass, "metadata.class", &metadata.class));
        }
        if let Some((_, field, code)) = cited
            .iter()
            .find(|(axis, _, code)| !vocabulary.has(*axis, code))
        {
            return Some(view.fail(
                "vocabulary_unknown_term",
                change(field),
                format!("{field} {code:?} is not in the vocabulary"),
            ));
        }
    }
    if view.kind == PartKind::PracticePack {
        if let Err(error) = manifest.pack_toolchain() {
            return Some(view.fail(
                "pack_toolchain_invalid",
                change("toolchain"),
                format!("toolchain is malformed ({})", error.code()),
            ));
        }
    }
    if !is_part_identifier(&metadata.id) || !matches(&SLUG, slug_segment(&metadata.id)) {
        return Some(view.fail(
            "id_grammar_invalid",
            identifier_fix(view),
            format!(
                "metadata.id {:?} does not match the identifier grammar",
                metadata.id
            ),
        ));
    }
    if let Some(loading) = &metadata.loading {
        if !is_loading_mode(loading) {
            return Some(view.fail(
                "loading_invalid",
                change("metadata.loading"),
                format!("metadata.loading {loading:?} is not a loading mode"),
            ));
        }
    }
    if let Some(status) = &metadata.status {
        if !vocabulary.has(VocabularyAxis::ArtefactStatus, status) {
            return Some(view.fail(
                "vocabulary_unknown_term",
                change("metadata.status"),
                format!("metadata.status {status:?} is not in the vocabulary"),
            ));
        }
    }
    if let Err(error) = manifest.required_practice_packs() {
        return Some(view.fail(
            "requiredpacks_invalid",
            change("requiredPracticePacks"),
            format!("requiredPracticePacks is malformed ({})", error.code()),
        ));
    }
    if let Some(problem) = assay_policy_problem(manifest) {
        return Some(view.fail("assay_policy_invalid", change("assayPolicy"), problem));
    }
    // spec: ordin-desktop/components/Checker#CHK-TP-1
    if view.kind == PartKind::WorkspaceProfile {
        const FIELD: &str = "toolPermissions.workspaceOverrides.prohibited";
        match manifest.prohibited_verbs() {
            Err(error) => {
                return Some(view.fail(
                    "tool_policy_invalid",
                    change(FIELD),
                    format!("{FIELD} is malformed ({})", error.code()),
                ));
            }
            Ok(verbs) => {
                if let Some(entry) = verbs.iter().find(|v| !matches(&VERB, v)) {
                    return Some(view.fail(
                        "tool_policy_invalid",
                        change(FIELD),
                        format!(
                            "prohibited entry {entry:?} is not a command (program or program.subcommand) or a tool name"
                        ),
                    ));
                }
            }
        }
    }
    None
}

/// The memory body checks (port of `memory.go`); they do not stop the part.
fn memory_checks(view: &PartView<'_>, vocabulary: &VocabularySet, out: &mut Vec<Finding>) {
    let contract = match view.manifest.memory_contract() {
        Ok(contract) => contract,
        Err(error) => {
            let code = match &error {
                AmError::FieldShapeInvalid { key, .. } if key.starts_with("retention") => {
                    "memory_retention_invalid"
                }
                AmError::FieldShapeInvalid { key, .. } if key.starts_with("store") => {
                    "memory_store_unresolved"
                }
                _ => "memory_content_class_invalid",
            };
            out.push(view.fail(
                code,
                change("memory"),
                format!("the memory contract is malformed ({})", error.code()),
            ));
            return;
        }
    };
    let Some(contract) = contract else {
        out.push(view.fail(
            "memory_retains_empty",
            change("retains"),
            "retains is empty".to_owned(),
        ));
        return;
    };
    if contract.retains.is_empty() {
        out.push(view.fail(
            "memory_retains_empty",
            change("retains"),
            "retains is empty".to_owned(),
        ));
    }
    for code in contract.retains.iter().chain(&contract.excludes) {
        if !vocabulary.has(VocabularyAxis::MemoryContentClass, code) {
            out.push(view.fail(
                "memory_content_class_invalid",
                change("retains"),
                format!("content class {code:?} is not in the vocabulary"),
            ));
        }
    }
    let retains: BTreeSet<&String> = contract.retains.iter().collect();
    for code in contract.excludes.iter().filter(|c| retains.contains(c)) {
        out.push(view.fail(
            "memory_class_conflict",
            change("excludes"),
            format!("content class {code:?} is both retained and excluded"),
        ));
    }
    let retention_ok = match (contract.max_age_days, contract.unbounded) {
        (Some(days), false) => days >= 1,
        (None, true) => true,
        _ => false,
    };
    if !retention_ok {
        out.push(view.fail(
            "memory_retention_invalid",
            change("retention"),
            "retention needs exactly one of maxAgeDays (at least 1) or unbounded".to_owned(),
        ));
    }
    let store_ok = match contract.store_location.as_str() {
        "local" => true,
        "remote" => contract.memory_service.is_some(),
        _ => false,
    };
    if !store_ok {
        out.push(view.fail(
            "memory_store_unresolved",
            change("store"),
            format!(
                "store.location {:?} is not local, or remote with a memoryService",
                contract.store_location
            ),
        ));
    }
}

/// The control-loop body checks (port of `controlloop.go`); they do not stop the part.
fn control_loop_checks(view: &PartView<'_>, vocabulary: &VocabularySet, out: &mut Vec<Finding>) {
    let manifest = view.manifest;
    for field in RETIRED_LOOP_FIELDS {
        if manifest.body.contains_key(field) {
            out.push(view.fail(
                "sync_control_loop_retired_field",
                vec![Fix::FixIt {
                    action: FixAction::DropUnknownField {
                        field: field.to_owned(),
                    },
                }],
                format!("the control loop field {field:?} is retired"),
            ));
        }
    }
    let spec = match manifest.control_loop() {
        Ok(Some(spec)) if !spec.strategy.trim().is_empty() => spec,
        Ok(_) => {
            out.push(view.fail(
                "sync_control_loop_missing_strategy",
                change("strategy"),
                "the control loop has no strategy".to_owned(),
            ));
            return;
        }
        Err(error) => {
            out.push(view.fail(
                "sync_control_loop_missing_strategy",
                change("strategy"),
                format!("the control loop is malformed ({})", error.code()),
            ));
            return;
        }
    };
    if !vocabulary.has(VocabularyAxis::LoopStrategy, &spec.strategy) {
        out.push(view.fail(
            "sync_unknown_loop_strategy",
            change("strategy"),
            format!("strategy {:?} is not in the vocabulary", spec.strategy),
        ));
    }
    for capability in &spec.requires {
        if !vocabulary.has(VocabularyAxis::RuntimeCapability, capability) {
            out.push(view.fail(
                "sync_unknown_runtime_capability",
                change("requires"),
                format!("runtime capability {capability:?} is not in the vocabulary"),
            ));
        }
    }
    if !matches(&TRIGGER, &spec.trigger) {
        out.push(view.fail(
            "sync_invalid_loop_trigger",
            change("trigger"),
            format!("trigger {:?} is not a valid trigger", spec.trigger),
        ));
    }
}

/// A lifecycle finding (`check-lifecycle-invalid`).
fn lifecycle_failure(view: &PartView<'_>, code: &str, field: &str, detail: String) -> Finding {
    finding(
        code,
        Severity::Failure,
        Some(view.id()),
        msg::LIFECYCLE_INVALID,
        &[("part", view.name())],
        change(field),
        detail,
    )
}

/// The stages a stage leads to: its `next` list, the following stage when `next` is absent.
pub(crate) fn successors(stages: &[JourneyStage], index: usize) -> Vec<String> {
    match &stages[index].next {
        Some(entries) => entries.iter().map(|e| e.stage.clone()).collect(),
        None => stages
            .get(index + 1)
            .map(|s| vec![s.id.clone()])
            .unwrap_or_default(),
    }
}

/// Step 8: the lifecycle form of a Journey part with `stages`, with no other part needed.
// A Journey with `seams` and no `stages` is the reference format's journey, not a
// lifecycle; the lifecycle checks apply only when the part has a `stages` key.
// spec: ordin-desktop/components/Checker#ADR-102
fn lifecycle_checks(view: &PartView<'_>, out: &mut Vec<Finding>) {
    if !view.manifest.body.contains_key("stages") {
        return;
    }
    let journey = match view.manifest.journey() {
        Ok(Some(journey)) => journey,
        Ok(None) => return,
        Err(error) => {
            out.push(lifecycle_failure(
                view,
                "lifecycle_stages_invalid",
                "stages",
                format!("the stages are malformed ({})", error.code()),
            ));
            return;
        }
    };
    let stages = &journey.stages;
    for stage in stages {
        if stage.name.trim().is_empty() || stage.role.trim().is_empty() {
            out.push(lifecycle_failure(
                view,
                "lifecycle_stage_unnamed",
                "stages",
                format!("stage {:?} has no name or no role", stage.id),
            ));
        }
    }
    let mut index_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut unique = true;
    for (i, stage) in stages.iter().enumerate() {
        if stage.id.trim().is_empty() || index_of.insert(stage.id.as_str(), i).is_some() {
            unique = false;
        }
    }
    if !unique || stages.is_empty() || stages.len() > MAX_STAGES {
        out.push(lifecycle_failure(
            view,
            "lifecycle_stages_invalid",
            "stages",
            format!(
                "a lifecycle has 1 to {MAX_STAGES} stages with unique ids; it has {} stages",
                stages.len()
            ),
        ));
        return;
    }
    // Reachability from the first stage.
    let mut seen = vec![false; stages.len()];
    let mut queue = vec![0usize];
    while let Some(i) = queue.pop() {
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        for next in successors(stages, i) {
            if let Some(&j) = index_of.get(next.as_str()) {
                queue.push(j);
            }
        }
    }
    for (i, stage) in stages.iter().enumerate() {
        if !seen[i] {
            out.push(lifecycle_failure(
                view,
                "lifecycle_stage_unreachable",
                "stages",
                format!(
                    "stage {:?} cannot be reached from the first stage",
                    stage.id
                ),
            ));
        }
        for next in successors(stages, i) {
            if index_of.get(next.as_str()).is_none_or(|&j| j <= i) {
                out.push(lifecycle_failure(
                    view,
                    "lifecycle_stage_backward",
                    "stages",
                    format!(
                        "stage {:?} goes on to {next:?}, which is not a later stage",
                        stage.id
                    ),
                ));
            }
        }
        if let Some(entries) = &stage.next {
            if !stage.together && entries.len() > 1 && !(2..=4).contains(&entries.len()) {
                out.push(lifecycle_failure(
                    view,
                    "lifecycle_choices_invalid",
                    "stages",
                    format!(
                        "stage {:?} offers {} choices, not 2 to 4",
                        stage.id,
                        entries.len()
                    ),
                ));
            }
        }
        for back in &stage.sends_back_to {
            let earlier = index_of.get(back.stage.as_str()).is_some_and(|&j| j < i);
            if !earlier || !(1..=10).contains(&back.max_rounds) {
                out.push(lifecycle_failure(
                    view,
                    "lifecycle_send_back_invalid",
                    "stages",
                    format!(
                        "stage {:?} sends back to {:?} with maxRounds {}: it must be an earlier stage, 1 to 10 rounds",
                        stage.id, back.stage, back.max_rounds
                    ),
                ));
            }
        }
        if let Some(days) = stage.answer_within_days {
            if !(1..=365).contains(&days) {
                out.push(lifecycle_failure(
                    view,
                    "lifecycle_days_invalid",
                    "stages",
                    format!(
                        "stage {:?} has answerWithinDays {days}, not 1 to 365",
                        stage.id
                    ),
                ));
            }
        }
        if let Some(channel) = &stage.slack_channel {
            let name_ok =
                (1..=80).contains(&channel.name.chars().count()) && !channel.name.starts_with('#');
            if !matches(&SLACK_CHANNEL_ID, &channel.id) || !name_ok {
                out.push(lifecycle_failure(
                    view,
                    "lifecycle_slack_channel_invalid",
                    "slackChannel",
                    format!("stage {:?} has a malformed slackChannel", stage.id),
                ));
            }
        }
    }
}

/// `check_part`: stage one of one part, then the body checks of its kind.
// spec: ordin-desktop/components/Checker#R-69
pub(crate) fn check(view: &PartView<'_>, vocabulary: &VocabularySet) -> Vec<Finding> {
    if let Some(stop) = stopping_rule(view, vocabulary) {
        return vec![stop];
    }
    let mut out = Vec::new();
    match view.kind {
        PartKind::Memory => memory_checks(view, vocabulary, &mut out),
        PartKind::ControlLoop => control_loop_checks(view, vocabulary, &mut out),
        PartKind::Journey => lifecycle_checks(view, &mut out),
        PartKind::Command => task_form_checks(view, &mut out),
        _ => {}
    }
    out
}

/// CHK-TF-1: a Command's title, inputs and the body that names them; each a failure that does not
/// stop the part, so at ingest no installed agent holds a task whose form cannot be built.
// spec: ordin-desktop/components/Checker#CHK-TF-1
fn task_form_checks(view: &PartView<'_>, out: &mut Vec<Finding>) {
    let manifest = view.manifest;
    let title = manifest.title();
    let task = match &title {
        Ok(Some(title)) => title.clone(),
        _ => view.name().to_owned(),
    };
    let fail = |code: &str, field: &str, detail: String| {
        finding(
            code,
            Severity::Failure,
            Some(view.id()),
            msg::TASK_FORM_INVALID,
            &[("task", &task)],
            change(field),
            detail,
        )
    };
    if let Err(error) = &title {
        out.push(fail(
            "task_title_invalid",
            "title",
            format!("title does not read: {}", error.code()),
        ));
    }
    let inputs = match manifest.inputs() {
        Ok(inputs) => inputs,
        Err(error) => {
            let detail = match &error {
                AmError::TaskInputInvalid { index, rule } => {
                    format!("inputs[{index}] breaks the rule {rule}")
                }
                other => format!("inputs do not read: {}", other.code()),
            };
            out.push(fail("task_inputs_invalid", "inputs", detail));
            return;
        }
    };
    if title.is_err() {
        return;
    }
    let Some(body) = &view.body else {
        return;
    };
    if let Err(AmError::TaskBodyInvalid { rule, name }) = check_task_body(inputs.as_deref(), body) {
        out.push(fail(
            "task_body_invalid",
            "inputs",
            format!("the body breaks the rule {rule} for {name}"),
        ));
    }
}

/// Every vocabulary term a part cites, as (axis, code): the metadata axes for every kind, and the
/// body axes of memory and control-loop parts.
pub(crate) fn cited_terms(kind: PartKind, manifest: &Manifest) -> Vec<(VocabularyAxis, String)> {
    let metadata = &manifest.metadata;
    let mut out = Vec::new();
    if !metadata.category.is_empty() {
        out.push((VocabularyAxis::Category, metadata.category.clone()));
    }
    if !metadata.class.is_empty() {
        out.push((VocabularyAxis::PackClass, metadata.class.clone()));
    }
    for role in &metadata.audience.roles {
        out.push((VocabularyAxis::AudienceRole, role.clone()));
    }
    if let Some(status) = &metadata.status {
        out.push((VocabularyAxis::ArtefactStatus, status.clone()));
    }
    if kind == PartKind::Memory {
        if let Ok(Some(contract)) = manifest.memory_contract() {
            for code in contract.retains.iter().chain(&contract.excludes) {
                out.push((VocabularyAxis::MemoryContentClass, code.clone()));
            }
        }
    }
    if kind == PartKind::ControlLoop {
        if let Ok(Some(spec)) = manifest.control_loop() {
            out.push((VocabularyAxis::LoopStrategy, spec.strategy.clone()));
            for code in spec.requires {
                out.push((VocabularyAxis::RuntimeCapability, code));
            }
        }
    }
    out
}

/// Codes a vocabulary part marks `state: retired`, by axis (W7). The typed reader drops the state,
/// so the raw `terms` list is read here.
pub(crate) fn retired_terms(manifest: &Manifest) -> Vec<(VocabularyAxis, String)> {
    let Ok(Some(body)) = manifest.vocabulary() else {
        return Vec::new();
    };
    let Some(items) = get(&manifest.body, "terms").and_then(as_seq) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(as_map)
        .filter(|term| str_at(term, "state").as_deref() == Some("retired"))
        .filter_map(|term| get(term, "code").and_then(text))
        .map(|code| (body.axis, code))
        .collect()
}
