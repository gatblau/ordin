//! A task's title and inputs (AgentModel.md amendment January 2026, AM-TF-1 to AM-TF-6; ADR-119).
//!
//! A Command part's front matter may carry `title` and `inputs` beside `argument-hint`. Both are body
//! keys, so the lossless round trip keeps them unchanged; this module only reads and checks them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::AmError;
use crate::manifest::{scalar_text, Manifest};
use crate::value::YamlValue;

/// One input of a task (ADR-119 item 1). Read from a Command part's front-matter `inputs`.
// spec: ordin-desktop/components/AgentModel#AM-TF-2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TaskInput {
    /// `^[a-z][a-z0-9_]{0,31}$`, unique within the task; the body names it as `${input:<name>}`.
    pub name: String,
    /// 1–60 characters after NFC normalisation, no control character.
    pub label: String,
    pub kind: TaskInputKind,
    /// `true` when the key is absent (ADR-119: "true unless stated").
    pub required: bool,
    /// 1–120 characters, shown under the field; None when absent.
    pub hint: Option<String>,
    /// Choice only: 2–20 distinct entries of 1–60 characters each, in the order written.
    pub choices: Vec<String>,
    /// Documents only: the most documents the field takes, 2–20. Required for documents.
    pub max: Option<u32>,
    /// The field's starting value as text. Not allowed on document and documents.
    pub default: Option<String>,
    /// Number only: the value is a whole number (a count or an ordinal). `false` when absent.
    // spec: ordin-desktop/components/AgentModel#AM-TF-2
    #[serde(default)]
    pub whole: bool,
}

/// The front-matter spellings are kebab-case: `short-text`, `long-text`, `document`, `documents`,
/// `choice`, `number`, `date`. The serde spelling below is the wire's (snake_case).
// spec: ordin-desktop/components/AgentModel#AM-TF-2
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TaskInputKind {
    ShortText,
    LongText,
    Document,
    Documents,
    Choice,
    Number,
    Date,
}

impl TaskInputKind {
    /// The kind of a front-matter spelling; None for any other text.
    pub fn from_front_matter(text: &str) -> Option<TaskInputKind> {
        Some(match text {
            "short-text" => TaskInputKind::ShortText,
            "long-text" => TaskInputKind::LongText,
            "document" => TaskInputKind::Document,
            "documents" => TaskInputKind::Documents,
            "choice" => TaskInputKind::Choice,
            "number" => TaskInputKind::Number,
            "date" => TaskInputKind::Date,
            _ => return None,
        })
    }

    /// The front-matter spelling of the kind.
    pub fn front_matter(self) -> &'static str {
        match self {
            TaskInputKind::ShortText => "short-text",
            TaskInputKind::LongText => "long-text",
            TaskInputKind::Document => "document",
            TaskInputKind::Documents => "documents",
            TaskInputKind::Choice => "choice",
            TaskInputKind::Number => "number",
            TaskInputKind::Date => "date",
        }
    }
}

pub const TASK_INPUT_NAME_PATTERN: &str = "^[a-z][a-z0-9_]{0,31}$";
/// A number as the core accepts it: an optional minus sign, 1–15 digits, and optionally a full stop
/// and 1–6 digits. The screens turn the person's locale's decimal separator into the full stop.
pub const NUMBER_PATTERN: &str = r"^-?[0-9]{1,15}(\.[0-9]{1,6})?$";
/// A whole number, for a number input with `whole: true`: an optional minus sign and 1–15 digits.
pub const WHOLE_NUMBER_PATTERN: &str = r"^-?[0-9]{1,15}$";
/// Lengths of a value, in Unicode scalar values after NFC normalisation.
pub const SHORT_TEXT_MAX: usize = 200;
pub const LONG_TEXT_MAX: usize = 4_000;
pub const TASK_INPUTS_MAX: usize = 8;
/// The fewest documents a filled documents field holds (ADR-119 item 1); one document is a document input.
pub const DOCUMENTS_MIN: usize = 2;

const TITLE_MAX: usize = 60;
const LABEL_MAX: usize = 60;
const HINT_MAX: usize = 120;
const CHOICE_MAX: usize = 60;
const CHOICES_MIN: usize = 2;
const CHOICES_MAX: usize = 20;
const DOCUMENTS_MAX_MIN: i64 = 2;
const DOCUMENTS_MAX_MAX: i64 = 20;
/// The keys an input may carry (AM-TF-3 step 2, rule `shape`).
const INPUT_KEYS: [&str; 9] = [
    "name", "label", "kind", "required", "hint", "choices", "max", "default", "whole",
];

fn name_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(TASK_INPUT_NAME_PATTERN).expect("static pattern"))
}

fn number_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(NUMBER_PATTERN).expect("static pattern"))
}

fn placeholder_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `${input:` then a name then `}`; the name is any run without `}` or white space, so an
    // unknown or misspelt name is still found and reported.
    RE.get_or_init(|| Regex::new(r"\$\{input:([^}\s]*)\}").expect("static pattern"))
}

/// True when `text` is a valid input name (`TASK_INPUT_NAME_PATTERN`).
pub fn is_task_input_name(text: &str) -> bool {
    name_regex().is_match(text)
}

/// True when `text` reads as a number (`NUMBER_PATTERN`).
pub fn is_task_number(text: &str) -> bool {
    number_regex().is_match(text)
}

/// True when `text` reads as a whole number (`WHOLE_NUMBER_PATTERN`).
pub fn is_task_whole_number(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    (1..=15).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

/// True when `text` is a calendar date written `YYYY-MM-DD`.
pub fn is_task_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digits = |s: &str| -> Option<u32> {
        if s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let (Some(year), Some(month), Some(day)) = (
        digits(&text[0..4]),
        digits(&text[5..7]),
        digits(&text[8..10]),
    ) else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// The NFC form of `text` and its length in scalar values; None when it holds a control character
/// (Unicode category Cc).
fn measured(text: &str) -> Option<(String, usize)> {
    if text.chars().any(char::is_control) {
        return None;
    }
    let normalised = ordin_types::text::normalise_text(text).ok()?;
    let count = normalised.chars().count();
    Some((normalised, count))
}

/// A string of 1..=max characters with no control character, NFC-normalised.
fn short_line(value: &YamlValue, max: usize) -> Option<String> {
    let text = scalar_text(value)?;
    let (normalised, count) = measured(&text)?;
    (1..=max).contains(&count).then_some(normalised)
}

fn input_error(index: usize, rule: &'static str) -> AmError {
    AmError::TaskInputInvalid {
        index: u32::try_from(index).unwrap_or(u32::MAX),
        rule,
    }
}

/// `map[key]`, treating an explicit null as absent.
fn get<'a>(map: &'a BTreeMap<String, YamlValue>, key: &str) -> Option<&'a YamlValue> {
    map.get(key).filter(|v| **v != YamlValue::Null)
}

fn read_input(
    index: usize,
    item: &YamlValue,
    seen: &mut BTreeSet<String>,
) -> Result<TaskInput, AmError> {
    let err = |rule| input_error(index, rule);
    // shape
    let map = item.as_map().ok_or_else(|| err("shape"))?;
    if map.keys().any(|k| !INPUT_KEYS.contains(&k.as_str())) {
        return Err(err("shape"));
    }
    // name, name_duplicate
    let name = match get(map, "name") {
        Some(YamlValue::Str(name)) if is_task_input_name(name) => name.clone(),
        _ => return Err(err("name")),
    };
    if !seen.insert(name.clone()) {
        return Err(err("name_duplicate"));
    }
    // label
    let label = get(map, "label")
        .and_then(|v| short_line(v, LABEL_MAX))
        .ok_or_else(|| err("label"))?;
    // kind
    let kind = match get(map, "kind") {
        Some(YamlValue::Str(text)) => TaskInputKind::from_front_matter(text),
        _ => None,
    }
    .ok_or_else(|| err("kind"))?;
    // required
    let required = match get(map, "required") {
        None => true,
        Some(YamlValue::Bool(b)) => *b,
        Some(_) => return Err(err("required")),
    };
    // hint
    let hint = match get(map, "hint") {
        None => None,
        Some(value) => Some(short_line(value, HINT_MAX).ok_or_else(|| err("hint"))?),
    };
    // choices
    let choices = match (kind, get(map, "choices")) {
        (TaskInputKind::Choice, Some(YamlValue::Seq(items))) => {
            if !(CHOICES_MIN..=CHOICES_MAX).contains(&items.len()) {
                return Err(err("choices"));
            }
            let mut out: Vec<String> = Vec::with_capacity(items.len());
            for entry in items {
                let text = short_line(entry, CHOICE_MAX).ok_or_else(|| err("choices"))?;
                if out.contains(&text) {
                    return Err(err("choices"));
                }
                out.push(text);
            }
            out
        }
        (TaskInputKind::Choice, _) => return Err(err("choices")),
        (_, Some(_)) => return Err(err("choices")),
        (_, None) => Vec::new(),
    };
    // max
    let max = match (kind, get(map, "max")) {
        (TaskInputKind::Documents, Some(YamlValue::Int(n)))
            if (DOCUMENTS_MAX_MIN..=DOCUMENTS_MAX_MAX).contains(n) =>
        {
            Some(u32::try_from(*n).map_err(|_| err("max"))?)
        }
        (TaskInputKind::Documents, _) => return Err(err("max")),
        (_, Some(_)) => return Err(err("max")),
        (_, None) => None,
    };
    // whole
    let whole = match (kind, get(map, "whole")) {
        (_, None) => false,
        (TaskInputKind::Number, Some(YamlValue::Bool(b))) => *b,
        (_, Some(_)) => return Err(err("whole")),
    };
    // default
    let default = match get(map, "default") {
        None => None,
        Some(value) => Some(
            read_default(kind, value, &choices)
                .filter(|text| !whole || is_task_whole_number(text))
                .ok_or_else(|| err("default"))?,
        ),
    };
    Ok(TaskInput {
        name,
        label,
        kind,
        required,
        hint,
        choices,
        max,
        default,
        whole,
    })
}

/// A default's text by its kind (AM-TF-3 step 2, rule `default`); None when it breaks the rule.
fn read_default(kind: TaskInputKind, value: &YamlValue, choices: &[String]) -> Option<String> {
    // A number or a date written unquoted is read as Int, Decimal or Str and converted to text.
    let text = scalar_text(value)?;
    match kind {
        TaskInputKind::Document | TaskInputKind::Documents => None,
        TaskInputKind::ShortText => {
            let (normalised, count) = measured(&text)?;
            (count <= SHORT_TEXT_MAX).then_some(normalised)
        }
        TaskInputKind::LongText => {
            if text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
            {
                return None;
            }
            let normalised = ordin_types::text::normalise_text(&text).ok()?;
            (normalised.chars().count() <= LONG_TEXT_MAX).then_some(normalised)
        }
        TaskInputKind::Choice => choices.contains(&text).then_some(text),
        TaskInputKind::Number => is_task_number(&text).then_some(text),
        TaskInputKind::Date => is_task_date(&text).then_some(text),
    }
}

impl Manifest {
    /// Command parts: the front-matter `title` (ADR-119 item 1). Absent → `Ok(None)`.
    // spec: ordin-desktop/components/AgentModel#AM-TF-3
    pub fn title(&self) -> Result<Option<String>, AmError> {
        match get(&self.body, "title") {
            None => Ok(None),
            Some(value) => {
                short_line(value, TITLE_MAX)
                    .map(Some)
                    .ok_or_else(|| AmError::FieldShapeInvalid {
                        key: "title".to_owned(),
                        expected: "string of 1 to 60 characters",
                    })
            }
        }
    }

    /// Command parts: the front-matter `inputs`. Absent → `Ok(None)`: the task keeps one
    /// long-text field from `argument-hint` (ADR-119 item 2). Present → one to eight inputs.
    // spec: ordin-desktop/components/AgentModel#AM-TF-3
    pub fn inputs(&self) -> Result<Option<Vec<TaskInput>>, AmError> {
        let Some(value) = get(&self.body, "inputs") else {
            return Ok(None);
        };
        let items = match value {
            YamlValue::Seq(items) if (1..=TASK_INPUTS_MAX).contains(&items.len()) => items,
            _ => {
                return Err(AmError::FieldShapeInvalid {
                    key: "inputs".to_owned(),
                    expected: "list of 1 to 8 inputs",
                })
            }
        };
        let mut seen = BTreeSet::new();
        items
            .iter()
            .enumerate()
            .map(|(index, item)| read_input(index, item, &mut seen))
            .collect::<Result<Vec<_>, _>>()
            .map(Some)
    }
}

/// The input names a body's placeholders name, in body order, each once.
pub fn task_placeholders(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for capture in placeholder_regex().captures_iter(body) {
        let name = capture[1].to_owned();
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Checks a Command's body against its inputs (AM-TF-4). `inputs` None: nothing to check.
// spec: ordin-desktop/components/AgentModel#AM-TF-4
pub fn check_task_body(inputs: Option<&[TaskInput]>, body: &str) -> Result<(), AmError> {
    let Some(inputs) = inputs else {
        return Ok(());
    };
    let named = task_placeholders(body);
    if let Some(unknown) = named
        .iter()
        .find(|name| !inputs.iter().any(|input| &input.name == *name))
    {
        return Err(AmError::TaskBodyInvalid {
            rule: "placeholder_unknown",
            name: unknown.clone(),
        });
    }
    if let Some(unused) = inputs.iter().find(|input| !named.contains(&input.name)) {
        return Err(AmError::TaskBodyInvalid {
            rule: "input_unused",
            name: unused.name.clone(),
        });
    }
    if body.contains("$ARGUMENTS") {
        return Err(AmError::TaskBodyInvalid {
            rule: "arguments_with_inputs",
            name: "$ARGUMENTS".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    fn command(front: &str) -> Manifest {
        let text = format!(
            "---\napiVersion: ai.enterprise/v1\nkind: command\nmetadata:\n  id: \"urn:ordin:default:commands:lifecycle:apply\"\n  name: /apply\n  version: 1.5.0\n  owner: gatblau\n  description: \"Applies a phase.\"\n{front}---\nbody\n"
        );
        parse_manifest("commands/lifecycle/apply.md", text.as_bytes())
            .expect("parses")
            .expect("a manifest")
    }

    #[test]
    fn dates() {
        assert!(is_task_date("2026-02-28"));
        assert!(is_task_date("2024-02-29"));
        assert!(!is_task_date("2026-02-29"));
        assert!(!is_task_date("2026-02-30"));
        assert!(!is_task_date("2026-13-01"));
        assert!(!is_task_date("2026-1-01"));
        assert!(!is_task_date("2026/01/01"));
    }

    #[test]
    fn numbers() {
        for ok in ["2", "-3", "3.5", "123456789012345", "1.123456"] {
            assert!(is_task_number(ok), "{ok}");
        }
        for bad in [
            "two",
            "3,5",
            "1.",
            ".5",
            "1.1234567",
            "+2",
            "1234567890123456",
        ] {
            assert!(!is_task_number(bad), "{bad}");
        }
    }

    #[test]
    fn names() {
        assert!(is_task_input_name("plan"));
        assert!(is_task_input_name("code_paths"));
        assert!(!is_task_input_name("Plan"));
        assert!(!is_task_input_name("_plan"));
        assert!(!is_task_input_name(&"a".repeat(33)));
    }

    #[test]
    fn title_rules() {
        assert_eq!(command("").title().unwrap(), None);
        assert_eq!(
            command("title: Apply a plan phase\n").title().unwrap(),
            Some("Apply a plan phase".to_owned())
        );
        let long = format!("title: {}\n", "a".repeat(61));
        assert!(matches!(
            command(&long).title(),
            Err(AmError::FieldShapeInvalid { ref key, .. }) if key == "title"
        ));
        assert!(command("title: [a]\n").title().is_err());
        assert!(command("title: \"\"\n").title().is_err());
        assert!(command("title: \"a\\nb\"\n").title().is_err());
    }

    #[test]
    fn input_defaults_by_kind() {
        let m = command(
            "inputs:\n  - name: a\n    label: A\n    kind: number\n    default: 2\n  - name: b\n    label: B\n    kind: date\n    default: 2026-01-31\n  - name: c\n    label: C\n    kind: choice\n    choices: [x, y]\n    default: y\n  - name: d\n    label: D\n    kind: long-text\n    required: false\n    hint: Some hint.\n",
        );
        let inputs = m.inputs().unwrap().unwrap();
        assert_eq!(inputs[0].default.as_deref(), Some("2"));
        assert_eq!(inputs[1].default.as_deref(), Some("2026-01-31"));
        assert_eq!(inputs[2].choices, vec!["x", "y"]);
        assert!(!inputs[3].required);
        assert_eq!(inputs[3].hint.as_deref(), Some("Some hint."));
    }

    // spec: ordin-desktop/components/AgentModel#AM-TF-3
    #[test]
    fn whole_numbers() {
        let m = command(
            "inputs:\n  - name: a\n    label: A\n    kind: number\n    whole: true\n    default: 3\n  - name: b\n    label: B\n    kind: number\n",
        );
        let inputs = m.inputs().unwrap().unwrap();
        assert!(inputs[0].whole);
        assert!(!inputs[1].whole);
        let rule = |yaml: &str| match command(yaml).inputs() {
            Err(AmError::TaskInputInvalid { rule, .. }) => rule,
            other => panic!("expected a rule, got {other:?}"),
        };
        assert_eq!(
            rule("inputs:\n  - name: a\n    label: A\n    kind: number\n    whole: true\n    default: 2.5\n"),
            "default"
        );
        assert_eq!(
            rule("inputs:\n  - name: a\n    label: A\n    kind: short-text\n    whole: true\n"),
            "whole"
        );
        assert_eq!(
            rule("inputs:\n  - name: a\n    label: A\n    kind: number\n    whole: yes please\n"),
            "whole"
        );
        assert!(is_task_whole_number("12"));
        assert!(is_task_whole_number("-3"));
        assert!(!is_task_whole_number("2.5"));
        assert!(!is_task_whole_number(""));
        assert!(!is_task_whole_number("-"));
        assert!(!is_task_whole_number("1234567890123456"));
    }

    #[test]
    fn body_rules() {
        let input = |name: &str| TaskInput {
            name: name.to_owned(),
            label: "L".to_owned(),
            kind: TaskInputKind::ShortText,
            required: true,
            hint: None,
            choices: Vec::new(),
            max: None,
            default: None,
            whole: false,
        };
        let inputs = vec![input("plan"), input("phase")];
        assert!(check_task_body(Some(&inputs), "${input:phase} ${input:plan}").is_ok());
        assert_eq!(
            check_task_body(Some(&inputs), "${input:plan} ${input:phase} ${input:step}"),
            Err(AmError::TaskBodyInvalid {
                rule: "placeholder_unknown",
                name: "step".to_owned()
            })
        );
        assert_eq!(
            check_task_body(Some(&inputs), "${input:plan}"),
            Err(AmError::TaskBodyInvalid {
                rule: "input_unused",
                name: "phase".to_owned()
            })
        );
        assert_eq!(
            check_task_body(
                Some(&inputs),
                "${input:plan} ${input:phase}\n```\n$ARGUMENTS\n```"
            ),
            Err(AmError::TaskBodyInvalid {
                rule: "arguments_with_inputs",
                name: "$ARGUMENTS".to_owned()
            })
        );
        assert!(check_task_body(None, "$ARGUMENTS").is_ok());
    }
}
