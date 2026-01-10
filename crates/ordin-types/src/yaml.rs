//! The one restricted YAML reader of the workspace (module `ordin_types::yaml`; Phase 4 decision P4-1).
//!
//! Every spec that reads a YAML file calls it and maps its three codes to its own; no crate parses
//! YAML another way. Anchors, aliases, merge keys and tags outside the YAML 1.2 core schema are
//! refused, so a small file cannot expand into a large tree, and the size is checked before parsing.

use std::borrow::Cow;

use saphyr_parser::{Event, Parser, ScalarStyle, ScanError, Span, Tag};
use serde::{Deserialize, Serialize};

use crate::error::{CodedError, RepairAction};

/// Nesting deeper than this many sequences and mappings is refused.
const MAX_DEPTH: usize = 64;
/// The handle of the YAML 1.2 core-schema tags once `!!` is resolved.
const CORE_HANDLE: &str = "tag:yaml.org,2002:";

/// Reads one YAML document into `T` under the restricted profile: no anchors, no aliases,
/// no tags other than the YAML 1.2 core-schema tags, at most `max_bytes` bytes of input.
pub fn read_restricted<T: serde::de::DeserializeOwned>(
    text: &str,
    max_bytes: usize,
) -> Result<T, YamlError> {
    let tree = read_tree(text, max_bytes)?;
    let value = to_json(&tree)?;
    // The document parsed but does not have the shape of `T`: not a line, the shape is wrong.
    serde_json::from_value::<T>(value).map_err(|_| YamlError::Malformed { line: 0 })
}

/// The same refusals and codes, returning an untyped tree whose scalars keep their exact source
/// text ("1.10" stays "1.10", "007" stays "007"); the Agent Model reads this form.
pub fn read_restricted_node(text: &str, max_bytes: usize) -> Result<YamlNode, YamlError> {
    Ok(to_node(read_tree(text, max_bytes)?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YamlNode {
    /// Keys in source order; each key is a scalar's source text.
    Map(Vec<(String, YamlNode)>),
    Seq(Vec<YamlNode>),
    /// `quoted` is true for single- and double-quoted scalars and block scalars; `text` is the
    /// scalar's value after YAML escape and folding rules, with no type resolution applied.
    Scalar {
        text: String,
        quoted: bool,
    },
    /// An empty value, `~` or plain `null`.
    Null,
}

/// `Display` never contains the input text, only the feature, the line (1-based) or the limit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum YamlError {
    /// An anchor (`&a`), an alias (`*a`), a merge key (`<<`) or a tag outside the core schema.
    #[error("the YAML uses a feature that is refused: {feature}")]
    FeatureRefused { feature: YamlFeature, line: u32 },
    /// Not well-formed YAML, more than one document, a duplicate or non-string mapping key,
    /// nesting deeper than 64, or a document that does not have the shape of `T`.
    #[error("the YAML is malformed at line {line}")]
    Malformed { line: u32 },
    /// `text.len() > max_bytes`.
    #[error("the YAML is larger than {max_bytes} bytes")]
    TooLarge { max_bytes: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum YamlFeature {
    Anchor,
    Alias,
    MergeKey,
    CustomTag,
}

impl std::fmt::Display for YamlFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            YamlFeature::Anchor => "anchor",
            YamlFeature::Alias => "alias",
            YamlFeature::MergeKey => "merge key",
            YamlFeature::CustomTag => "custom tag",
        })
    }
}

impl YamlError {
    /// `YAML_FEATURE_REFUSED` | `YAML_MALFORMED` | `YAML_TOO_LARGE`.
    pub fn code(&self) -> &'static str {
        match self {
            YamlError::FeatureRefused { .. } => "YAML_FEATURE_REFUSED",
            YamlError::Malformed { .. } => "YAML_MALFORMED",
            YamlError::TooLarge { .. } => "YAML_TOO_LARGE",
        }
    }
}

/// No YAML refusal is retryable: each is a property of the text.
impl CodedError for YamlError {
    fn code(&self) -> &'static str {
        YamlError::code(self)
    }
    fn retryable(&self) -> bool {
        false
    }
    fn repair(&self) -> Option<RepairAction> {
        None
    }
}

// ---- the checked tree ----

/// A core-schema tag named on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoreTag {
    Str,
    Int,
    Float,
    Bool,
    Null,
    Map,
    Seq,
}

/// The tree after steps (a)–(d): every refusal applied, scalars still untyped.
enum Tree {
    Map(Vec<(String, Tree)>),
    Seq(Vec<Tree>),
    Scalar {
        text: String,
        style: ScalarStyle,
        tag: Option<CoreTag>,
        line: u32,
    },
    /// The empty stream.
    Empty,
}

fn line_of(span: &Span) -> u32 {
    u32::try_from(span.start.line()).unwrap_or(u32::MAX)
}

fn scan_error(error: &ScanError) -> YamlError {
    YamlError::Malformed {
        line: u32::try_from(error.marker().line()).unwrap_or(u32::MAX),
    }
}

/// Maps a node's tag to a core-schema tag, or refuses it as custom.
fn core_tag(tag: Option<Cow<'_, Tag>>, line: u32) -> Result<Option<CoreTag>, YamlError> {
    let Some(tag) = tag else {
        return Ok(None);
    };
    // `!!str` arrives as handle `tag:yaml.org,2002:` with suffix `str`; the verbatim form
    // `!<tag:yaml.org,2002:str>` arrives with the whole name in one part.
    let full = format!("{}{}", tag.handle, tag.suffix);
    let suffix = full.strip_prefix(CORE_HANDLE).unwrap_or("");
    let core = match suffix {
        "str" => CoreTag::Str,
        "int" => CoreTag::Int,
        "float" => CoreTag::Float,
        "bool" => CoreTag::Bool,
        "null" => CoreTag::Null,
        "map" => CoreTag::Map,
        "seq" => CoreTag::Seq,
        _ => {
            return Err(YamlError::FeatureRefused {
                feature: YamlFeature::CustomTag,
                line,
            })
        }
    };
    Ok(Some(core))
}

fn refuse_anchor(anchor_id: usize, line: u32) -> Result<(), YamlError> {
    if anchor_id == 0 {
        Ok(())
    } else {
        Err(YamlError::FeatureRefused {
            feature: YamlFeature::Anchor,
            line,
        })
    }
}

/// Pulls events and builds the checked tree of the one document (Internal Logic 10 (a)–(d)).
struct Builder<'input> {
    parser: Parser<'input, saphyr_parser::StrInput<'input>>,
}

impl<'input> Builder<'input> {
    fn next(&mut self) -> Result<(Event<'input>, Span), YamlError> {
        match self.parser.next() {
            Some(Ok(pair)) => Ok(pair),
            Some(Err(error)) => Err(scan_error(&error)),
            // The parser always ends with `StreamEnd`; running out early is malformed input.
            None => Err(YamlError::Malformed { line: 0 }),
        }
    }

    /// The whole stream: at most one document.
    fn stream(&mut self) -> Result<Tree, YamlError> {
        let mut tree = Tree::Empty;
        let mut documents = 0usize;
        loop {
            let (event, span) = self.next()?;
            match event {
                Event::StreamStart | Event::Nothing => {}
                Event::StreamEnd => return Ok(tree),
                Event::DocumentStart(_) => {
                    documents += 1;
                    if documents > 1 {
                        return Err(YamlError::Malformed {
                            line: line_of(&span),
                        });
                    }
                    let (first, first_span) = self.next()?;
                    tree = self.node(first, first_span, 0)?;
                }
                Event::DocumentEnd => {}
                _ => {
                    return Err(YamlError::Malformed {
                        line: line_of(&span),
                    })
                }
            }
        }
    }

    /// One node starting at `event`; `depth` counts the enclosing sequences and mappings.
    fn node(&mut self, event: Event<'input>, span: Span, depth: usize) -> Result<Tree, YamlError> {
        let line = line_of(&span);
        match event {
            Event::Alias(_) => Err(YamlError::FeatureRefused {
                feature: YamlFeature::Alias,
                line,
            }),
            Event::Scalar(text, style, anchor_id, tag) => {
                refuse_anchor(anchor_id, line)?;
                let tag = core_tag(tag, line)?;
                Ok(Tree::Scalar {
                    text: text.into_owned(),
                    style,
                    tag,
                    line,
                })
            }
            Event::SequenceStart(anchor_id, tag) => {
                refuse_anchor(anchor_id, line)?;
                let tag = core_tag(tag, line)?;
                if !matches!(tag, None | Some(CoreTag::Seq)) || depth >= MAX_DEPTH {
                    return Err(YamlError::Malformed { line });
                }
                let mut items = Vec::new();
                loop {
                    let (next, next_span) = self.next()?;
                    if next == Event::SequenceEnd {
                        return Ok(Tree::Seq(items));
                    }
                    items.push(self.node(next, next_span, depth + 1)?);
                }
            }
            Event::MappingStart(anchor_id, tag) => {
                refuse_anchor(anchor_id, line)?;
                let tag = core_tag(tag, line)?;
                if !matches!(tag, None | Some(CoreTag::Map)) || depth >= MAX_DEPTH {
                    return Err(YamlError::Malformed { line });
                }
                let mut entries: Vec<(String, Tree)> = Vec::new();
                loop {
                    let (next, next_span) = self.next()?;
                    if next == Event::MappingEnd {
                        return Ok(Tree::Map(entries));
                    }
                    let key_line = line_of(&next_span);
                    let key = match self.node(next, next_span, depth + 1)? {
                        Tree::Scalar { text, style, .. } => {
                            if style == ScalarStyle::Plain && text == "<<" {
                                return Err(YamlError::FeatureRefused {
                                    feature: YamlFeature::MergeKey,
                                    line: key_line,
                                });
                            }
                            text
                        }
                        // A sequence or mapping used as a key.
                        _ => return Err(YamlError::Malformed { line: key_line }),
                    };
                    if entries.iter().any(|(seen, _)| *seen == key) {
                        return Err(YamlError::Malformed { line: key_line });
                    }
                    let (value, value_span) = self.next()?;
                    let value = self.node(value, value_span, depth + 1)?;
                    entries.push((key, value));
                }
            }
            _ => Err(YamlError::Malformed { line }),
        }
    }
}

/// Steps (a)–(d): the size, then the parse with every refusal.
fn read_tree(text: &str, max_bytes: usize) -> Result<Tree, YamlError> {
    if text.len() > max_bytes {
        return Err(YamlError::TooLarge { max_bytes });
    }
    let mut builder = Builder {
        parser: Parser::new_from_str(text),
    };
    builder.stream()
}

// ---- step (e), the node form ----

fn is_plain_null(text: &str, style: ScalarStyle) -> bool {
    style == ScalarStyle::Plain && matches!(text, "" | "~" | "null")
}

fn to_node(tree: Tree) -> YamlNode {
    match tree {
        Tree::Empty => YamlNode::Null,
        Tree::Map(entries) => YamlNode::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key, to_node(value)))
                .collect(),
        ),
        Tree::Seq(items) => YamlNode::Seq(items.into_iter().map(to_node).collect()),
        Tree::Scalar { text, style, .. } => {
            if is_plain_null(&text, style) {
                YamlNode::Null
            } else {
                YamlNode::Scalar {
                    quoted: style != ScalarStyle::Plain,
                    text,
                }
            }
        }
    }
}

// ---- step (e), the typed form: YAML 1.2 core-schema resolution ----

fn to_json(tree: &Tree) -> Result<serde_json::Value, YamlError> {
    use serde_json::Value;
    match tree {
        Tree::Empty => Ok(Value::Null),
        Tree::Map(entries) => {
            let mut map = serde_json::Map::with_capacity(entries.len());
            for (key, value) in entries {
                map.insert(key.clone(), to_json(value)?);
            }
            Ok(Value::Object(map))
        }
        Tree::Seq(items) => items
            .iter()
            .map(to_json)
            .collect::<Result<_, _>>()
            .map(Value::Array),
        Tree::Scalar {
            text,
            style,
            tag,
            line,
        } => resolve_scalar(text, *style, *tag, *line),
    }
}

fn resolve_scalar(
    text: &str,
    style: ScalarStyle,
    tag: Option<CoreTag>,
    line: u32,
) -> Result<serde_json::Value, YamlError> {
    use serde_json::Value;
    let malformed = YamlError::Malformed { line };
    match tag {
        Some(CoreTag::Str) => Ok(Value::String(text.to_owned())),
        Some(CoreTag::Null) => core_null(text).then_some(Value::Null).ok_or(malformed),
        Some(CoreTag::Bool) => core_bool(text).map(Value::Bool).ok_or(malformed),
        Some(CoreTag::Int) => match core_int(text) {
            Some(Ok(n)) => Ok(Value::from(n)),
            _ => Err(malformed),
        },
        Some(CoreTag::Float) => {
            let number = match core_int(text) {
                Some(Ok(n)) => Some(n as f64),
                Some(Err(())) => None,
                None => core_float(text),
            };
            finite(number, line)
        }
        // A collection tag on a scalar.
        Some(CoreTag::Map) | Some(CoreTag::Seq) => Err(malformed),
        // Quoted and block scalars are strings.
        None if style != ScalarStyle::Plain => Ok(Value::String(text.to_owned())),
        None => {
            if core_null(text) {
                return Ok(Value::Null);
            }
            if let Some(b) = core_bool(text) {
                return Ok(Value::Bool(b));
            }
            match core_int(text) {
                Some(Ok(n)) => return Ok(Value::from(n)),
                // SPEC-GAP-8: an integer outside i64 has no rule in SharedTypes.md Internal Logic 10;
                // it is refused as malformed rather than silently read as text or as a float.
                Some(Err(())) => return Err(malformed),
                None => {}
            }
            match core_float(text) {
                Some(f) => finite(Some(f), line),
                None => Ok(Value::String(text.to_owned())),
            }
        }
    }
}

/// A finite float becomes a JSON number; `.inf` and `.nan` are refused, because JSON cannot carry them.
fn finite(number: Option<f64>, line: u32) -> Result<serde_json::Value, YamlError> {
    number
        .and_then(serde_json::Number::from_f64)
        .map(serde_json::Value::Number)
        .ok_or(YamlError::Malformed { line })
}

fn core_null(text: &str) -> bool {
    matches!(text, "" | "~" | "null" | "Null" | "NULL")
}

fn core_bool(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

/// `None` when `text` is not a core-schema integer; `Some(Err)` when it is one that does not fit i64.
fn core_int(text: &str) -> Option<Result<i64, ()>> {
    let all =
        |digits: &str, ok: fn(&u8) -> bool| !digits.is_empty() && digits.bytes().all(|b| ok(&b));
    if let Some(octal) = text.strip_prefix("0o") {
        return all(octal, |b| (b'0'..=b'7').contains(b))
            .then(|| i64::from_str_radix(octal, 8).map_err(|_| ()));
    }
    if let Some(hex) = text.strip_prefix("0x") {
        return all(hex, u8::is_ascii_hexdigit)
            .then(|| i64::from_str_radix(hex, 16).map_err(|_| ()));
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    all(unsigned, u8::is_ascii_digit).then(|| text.parse::<i64>().map_err(|_| ()))
}

/// A core-schema float, `.inf` and `.nan` included (the caller refuses the non-finite ones).
fn core_float(text: &str) -> Option<f64> {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        return Some(if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    if matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Some(f64::NAN);
    }
    // `[-+]? ( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty())
        }
        None => !mantissa.is_empty() && digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    if mantissa_ok && exponent_ok {
        text.parse::<f64>().ok()
    } else {
        None
    }
}
