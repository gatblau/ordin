//! File placement, rendering and the fixed YAML emitter (AgentModel.md *Internal Logic* 8–9).

use ordin_types::domain::agents::PartKind;

use crate::error::AmError;
use crate::ident::Urn;
use crate::kinds::{kind_info, FileConvention};
use crate::manifest::{Manifest, Metadata};
use crate::value::{is_decimal_text, is_int_text, YamlValue};

/// One node of the emitter: maps keep the order the caller gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Node {
    Null,
    Bool(bool),
    Int(i64),
    /// Written as its text.
    Raw(String),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<(String, Node)>),
}

impl Node {
    pub(crate) fn str(text: &str) -> Node {
        Node::Str(text.to_owned())
    }

    pub(crate) fn strs(items: &[String]) -> Node {
        Node::Seq(items.iter().map(|s| Node::str(s)).collect())
    }
}

/// `YamlValue` as an emitter node; map keys stay sorted, as `BTreeMap` holds them.
pub(crate) fn node_of(value: &YamlValue) -> Node {
    match value {
        YamlValue::Null => Node::Null,
        YamlValue::Bool(b) => Node::Bool(*b),
        YamlValue::Int(n) => Node::Int(*n),
        YamlValue::Decimal(text) => Node::Raw(text.clone()),
        YamlValue::Str(text) => Node::Str(text.clone()),
        YamlValue::Seq(items) => Node::Seq(items.iter().map(node_of).collect()),
        YamlValue::Map(map) => {
            Node::Map(map.iter().map(|(k, v)| (k.clone(), node_of(v))).collect())
        }
    }
}

const RESERVED: [&str; 8] = ["true", "false", "yes", "no", "on", "off", "null", "~"];

/// True when `text` may be written as a plain scalar (step 9).
fn is_plain(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    let first_ok = first.is_ascii_alphanumeric() || matches!(first, '_' | '.' | '/' | '@');
    let rest_ok = chars.all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '/' | '@' | ':' | '+' | '-')
    });
    // SPEC-GAP-11: the plain pattern of step 9 lets a string start with `@`, but YAML reserves `@` as
    // an indicator that cannot start a plain scalar, so such a string would not read back. A string
    // starting with `@` (a legacy identifier) is written double-quoted.
    first_ok
        && first != '@'
        && rest_ok
        && !text.ends_with(' ')
        && !text.ends_with(':')
        && !text.contains(": ")
        && !text.contains(" #")
        && !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(text))
        && !is_int_text(text)
        && !is_decimal_text(text)
}

/// A string scalar: plain when allowed, otherwise double-quoted with JSON escaping.
pub(crate) fn scalar_string(text: &str) -> String {
    if is_plain(text) {
        text.to_owned()
    } else {
        // Serialising a `&str` to JSON cannot fail.
        serde_json::to_string(text).unwrap_or_default()
    }
}

fn inline(node: &Node) -> Option<String> {
    Some(match node {
        Node::Null => "null".to_owned(),
        Node::Bool(b) => b.to_string(),
        Node::Int(n) => n.to_string(),
        Node::Raw(text) => text.clone(),
        Node::Str(text) => scalar_string(text),
        Node::Seq(items) if items.is_empty() => "[]".to_owned(),
        Node::Map(entries) if entries.is_empty() => "{}".to_owned(),
        Node::Seq(_) | Node::Map(_) => return None,
    })
}

/// Writes the entries of a map, each line at `indent` spaces.
fn emit_map(entries: &[(String, Node)], indent: usize, out: &mut String) {
    for (key, value) in entries {
        out.push_str(&" ".repeat(indent));
        emit_entry(key, value, indent, out);
    }
}

/// `key: value` whose first line is already indented.
fn emit_entry(key: &str, value: &Node, indent: usize, out: &mut String) {
    out.push_str(&scalar_string(key));
    out.push(':');
    match inline(value) {
        Some(text) => {
            out.push(' ');
            out.push_str(&text);
            out.push('\n');
        }
        None => {
            out.push('\n');
            match value {
                Node::Map(entries) => emit_map(entries, indent + 2, out),
                Node::Seq(items) => emit_seq(items, indent + 2, out),
                _ => {}
            }
        }
    }
}

/// Writes `- ` items, each dash at `indent` spaces.
fn emit_seq(items: &[Node], indent: usize, out: &mut String) {
    for item in items {
        out.push_str(&" ".repeat(indent));
        out.push('-');
        match inline(item) {
            Some(text) => {
                out.push(' ');
                out.push_str(&text);
                out.push('\n');
            }
            None => match item {
                Node::Map(entries) => {
                    // The first key on the dash line, the rest aligned under it.
                    out.push(' ');
                    for (i, (key, value)) in entries.iter().enumerate() {
                        if i > 0 {
                            out.push_str(&" ".repeat(indent + 2));
                        }
                        emit_entry(key, value, indent + 2, out);
                    }
                }
                Node::Seq(nested) => {
                    out.push('\n');
                    emit_seq(nested, indent + 2, out);
                }
                _ => out.push('\n'),
            },
        }
    }
}

/// The document of a top-level map: two-space indent, LF, one trailing newline.
pub(crate) fn emit_document(entries: &[(String, Node)]) -> String {
    let mut out = String::new();
    emit_map(entries, 0, &mut out);
    if out.is_empty() {
        out.push_str("{}\n");
    }
    out
}

/// The typed metadata keys in the order of `Metadata`'s fields, then `extra` sorted; empty values
/// are not written.
pub(crate) fn metadata_node(metadata: &Metadata) -> Node {
    let mut entries = vec![("id".to_owned(), Node::str(&metadata.id))];
    let mut text = |key: &str, value: &str| {
        if !value.is_empty() {
            entries.push((key.to_owned(), Node::str(value)));
        }
    };
    text("name", &metadata.name);
    text("version", &metadata.version);
    text("owner", &metadata.owner);
    text("loading", metadata.loading.as_deref().unwrap_or(""));
    text("description", &metadata.description);
    text("category", &metadata.category);
    text("class", &metadata.class);
    let audience = &metadata.audience;
    let audience_entries: Vec<(String, Node)> = [
        ("domains", &audience.domains),
        ("roles", &audience.roles),
        ("languages", &audience.languages),
    ]
    .into_iter()
    .filter(|(_, list)| !list.is_empty())
    .map(|(key, list)| (key.to_owned(), Node::strs(list)))
    .collect();
    if !audience_entries.is_empty() {
        entries.push(("audience".to_owned(), Node::Map(audience_entries)));
    }
    if !metadata.use_cases.is_empty() {
        let cases = metadata
            .use_cases
            .iter()
            .map(|case| {
                Node::Map(vec![
                    ("title".to_owned(), Node::str(&case.title)),
                    ("summary".to_owned(), Node::str(&case.summary)),
                ])
            })
            .collect();
        entries.push(("useCases".to_owned(), Node::Seq(cases)));
    }
    if let Some(status) = &metadata.status {
        entries.push(("status".to_owned(), Node::str(status)));
    }
    if let Some(supersedes) = &metadata.supersedes {
        entries.push(("supersedes".to_owned(), Node::str(supersedes)));
    }
    for (key, value) in &metadata.extra {
        entries.push((key.clone(), node_of(value)));
    }
    Node::Map(entries)
}

/// The whole manifest as YAML: `apiVersion`, `kind`, `metadata`, then `body` keys sorted.
pub fn render_manifest_yaml(manifest: &Manifest) -> String {
    let mut entries = vec![
        ("apiVersion".to_owned(), Node::str(&manifest.api_version)),
        ("kind".to_owned(), Node::str(&manifest.authored_kind)),
        ("metadata".to_owned(), metadata_node(&manifest.metadata)),
    ];
    for (key, value) in &manifest.body {
        entries.push((key.clone(), node_of(value)));
    }
    emit_document(&entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub path: String,
    pub content: Vec<u8>,
}

fn convention(kind: PartKind) -> Result<FileConvention, AmError> {
    kind_info(kind)
        .file_convention
        .ok_or(AmError::NoFileConvention { kind })
}

/// Where the Forge places a part's file, from its URN.
pub fn part_file_path(kind: PartKind, id: &str) -> Result<String, AmError> {
    let urn = Urn::parse(id).ok_or_else(|| AmError::IdInvalid { id: id.to_owned() })?;
    let convention = convention(kind)?;
    let mut base = format!("models/@{}/{}", urn.publisher, urn.kind_dir);
    if !urn.categories.is_empty() {
        base.push('/');
        base.push_str(&urn.categories.join("/"));
    }
    let slug = &urn.slug;
    Ok(match convention {
        FileConvention::MarkdownFile => format!("{base}/{slug}.md"),
        FileConvention::SkillDirectory => format!("{base}/{slug}/SKILL.md"),
        FileConvention::YamlFile => format!("{base}/{slug}.yaml"),
        FileConvention::YamlInDirectory { file_name } => format!("{base}/{slug}/{file_name}"),
        FileConvention::ControlLoopFile => format!("{base}/control-loop-{slug}.md"),
    })
}

/// Front matter + body for Markdown conventions, the whole manifest for YAML ones.
pub fn render_part(
    kind: PartKind,
    manifest: &Manifest,
    body: Option<&str>,
) -> Result<RenderedFile, AmError> {
    let convention = convention(kind)?;
    let path = part_file_path(kind, &manifest.metadata.id)?;
    let yaml = render_manifest_yaml(manifest);
    let body = body.unwrap_or("");
    let content = if convention.is_markdown() {
        let mut text = format!("---\n{yaml}---\n{body}");
        if !body.is_empty() && !body.ends_with('\n') {
            text.push('\n');
        }
        text
    } else {
        if !body.is_empty() {
            return Err(AmError::FieldShapeInvalid {
                key: "body".to_owned(),
                expected: "no body for a YAML file",
            });
        }
        yaml
    };
    Ok(RenderedFile {
        path,
        content: content.into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    #[test]
    fn plain_rule() {
        for plain in [
            "urn:ordin:default:skills:x",
            "1.4.0",
            "Code generation",
            "sha256:00ab",
            "github.com/acme/x",
            "a+b",
        ] {
            assert_eq!(scalar_string(plain), plain, "{plain}");
        }
        for quoted in [
            "", "1.0", "42", "Yes", "~", "a: b", "a #b", "end:", "end ", "-dash", "@a/b", "é",
            "a\nb", "<x>",
        ] {
            assert!(scalar_string(quoted).starts_with('"'), "{quoted}");
        }
    }

    #[test]
    fn emitter_shapes() {
        let doc = emit_document(&[
            ("a".into(), Node::Seq(vec![])),
            ("b".into(), Node::Map(vec![])),
            (
                "c".into(),
                Node::Seq(vec![
                    Node::Map(vec![
                        ("x".into(), Node::Int(1)),
                        ("y".into(), Node::Seq(vec![Node::Bool(true)])),
                    ]),
                    Node::Seq(vec![Node::Null, Node::Raw("1.10".into())]),
                ]),
            ),
        ]);
        assert_eq!(
            doc,
            "a: []\nb: {}\nc:\n  - x: 1\n    y:\n      - true\n  -\n    - null\n    - 1.10\n"
        );
        assert_eq!(emit_document(&[]), "{}\n");
        // The emitted text reads back to the same values.
        let parsed = crate::value::read_value(doc.as_bytes()).unwrap();
        let c = parsed.as_map().unwrap()["c"].as_seq().unwrap();
        assert_eq!(
            c[1],
            YamlValue::Seq(vec![YamlValue::Null, YamlValue::Decimal("1.10".into())])
        );
    }

    #[test]
    fn placement() {
        let cases = [
            (
                PartKind::Skill,
                "urn:ordin:default:skills:quality:codegen",
                "models/@default/skills/quality/codegen/SKILL.md",
            ),
            (
                PartKind::Rule,
                "urn:ordin:default:rules:layer:sql",
                "models/@default/rules/layer/sql.md",
            ),
            (
                PartKind::Capability,
                "urn:ordin:a:capabilities:c",
                "models/@a/capabilities/c.yaml",
            ),
            (
                PartKind::WorkspaceProfile,
                "urn:ordin:a:workspaces:w",
                "models/@a/workspaces/w/workspace-profile.yaml",
            ),
            (
                PartKind::PracticePack,
                "urn:ordin:a:practice-packs:stack:go",
                "models/@a/practice-packs/stack/go/practice-pack.yaml",
            ),
            (
                PartKind::ControlLoop,
                "urn:ordin:a:rules:control-loops:cspa",
                "models/@a/rules/control-loops/control-loop-cspa.md",
            ),
            (
                PartKind::SystemRole,
                "urn:ordin:a:workspaces:w:system-role",
                "models/@a/workspaces/w/system-role.md",
            ),
        ];
        for (kind, id, path) in cases {
            assert_eq!(part_file_path(kind, id).unwrap(), path);
        }
        assert_eq!(
            part_file_path(PartKind::Skill, "@a/skills/x")
                .unwrap_err()
                .code(),
            "AM_ID_INVALID"
        );
    }

    #[test]
    fn render_round_trip() {
        let manifest = parse_manifest(
            "x.yaml",
            b"apiVersion: ai.enterprise/v1\nkind: capability\nmetadata:\n  id: urn:ordin:a:capabilities:c\n  name: C\n  audience: {roles: [dev]}\n  useCases:\n    - {title: T, summary: S}\n  status: draft\n  supersedes: urn:ordin:a:capabilities:b\n  tags: [x]\nskills: ['@a/skills/s']\n",
        )
        .unwrap()
        .unwrap();
        let file = render_part(PartKind::Capability, &manifest, None).unwrap();
        let back = parse_manifest(&file.path, &file.content).unwrap().unwrap();
        assert_eq!(back, manifest);
        assert_eq!(
            render_part(PartKind::Capability, &manifest, Some("text"))
                .unwrap_err()
                .code(),
            "AM_FIELD_SHAPE_INVALID"
        );
        let mut rule = manifest.clone();
        rule.metadata.id = "urn:ordin:a:rules:r".into();
        let file = render_part(PartKind::Rule, &rule, Some("Body")).unwrap();
        assert!(String::from_utf8(file.content)
            .unwrap()
            .ends_with("---\nBody\n"));
        assert_eq!(
            render_part(PartKind::CrewProfile, &manifest, None).unwrap_err(),
            AmError::NoFileConvention {
                kind: PartKind::CrewProfile
            }
        );
    }
}
