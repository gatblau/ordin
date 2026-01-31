//! Markdown to and from Atlassian Document Format (JiraAdapter.md Internal Logic 8). Pure; no
//! network. Text read from Jira is data: nothing here interprets it.

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde_json::{json, Map, Value};

use crate::{JiraCodeBlock, JiraError};

/// ADF nests no deeper than this on the way back; deeper content is read as its text.
const MAX_DEPTH: usize = 64;

// ---------------------------------------------------------------- Markdown to ADF

/// One open block while the Markdown is walked.
struct Open {
    kind: &'static str,
    attrs: Option<Value>,
    content: Vec<Value>,
    /// A paragraph opened only to hold the loose text of a tight list item.
    implicit: bool,
}

#[derive(Clone, PartialEq)]
enum Mark {
    Strong,
    Em,
    Strike,
    Link(String),
}

impl Mark {
    fn to_json(&self) -> Value {
        match self {
            Mark::Strong => json!({"type": "strong"}),
            Mark::Em => json!({"type": "em"}),
            Mark::Strike => json!({"type": "strike"}),
            Mark::Link(href) => json!({"type": "link", "attrs": {"href": href}}),
        }
    }
}

struct Builder {
    stack: Vec<Open>,
    marks: Vec<Mark>,
    /// The text of the code block being read.
    code: Option<(Option<String>, String)>,
}

impl Builder {
    fn open(&mut self, kind: &'static str, attrs: Option<Value>) {
        self.stack.push(Open {
            kind,
            attrs,
            content: Vec::new(),
            implicit: false,
        });
    }

    fn close_implicit(&mut self) {
        if self.stack.last().is_some_and(|o| o.implicit) {
            self.close();
        }
    }

    fn close(&mut self) {
        if let Some(open) = self.stack.pop() {
            let mut node = Map::new();
            node.insert("type".into(), Value::String(open.kind.to_owned()));
            if let Some(attrs) = open.attrs {
                node.insert("attrs".into(), attrs);
            }
            if !open.content.is_empty() {
                node.insert("content".into(), Value::Array(open.content));
            }
            self.push_node(Value::Object(node));
        }
    }

    fn push_node(&mut self, node: Value) {
        if let Some(top) = self.stack.last_mut() {
            top.content.push(node);
        }
    }

    /// Inline content needs a paragraph or heading around it; a tight list item has none.
    fn ensure_inline_parent(&mut self) {
        let inline_parent = self
            .stack
            .last()
            .is_some_and(|o| o.kind == "paragraph" || o.kind == "heading");
        if !inline_parent {
            self.open("paragraph", None);
            if let Some(top) = self.stack.last_mut() {
                top.implicit = true;
            }
        }
    }

    fn marks_json(&self, extra: Option<Value>) -> Option<Value> {
        let mut marks: Vec<Value> = self
            .marks
            .iter()
            .filter(|m| !matches!(m, Mark::Link(href) if href.is_empty()))
            .map(Mark::to_json)
            .collect();
        if let Some(extra) = extra {
            marks.push(extra);
        }
        (!marks.is_empty()).then_some(Value::Array(marks))
    }

    /// Adds text, merging with the previous text node when the marks are the same.
    fn text(&mut self, text: &str, extra: Option<Value>) {
        if text.is_empty() {
            return;
        }
        self.ensure_inline_parent();
        let marks = self.marks_json(extra);
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        if let Some(Value::Object(last)) = top.content.last_mut() {
            let same_type = last.get("type").and_then(Value::as_str) == Some("text");
            if same_type && last.get("marks") == marks.as_ref() {
                if let Some(Value::String(existing)) = last.get_mut("text") {
                    existing.push_str(text);
                    return;
                }
            }
        }
        let mut node = Map::new();
        node.insert("type".into(), Value::String("text".into()));
        node.insert("text".into(), Value::String(text.to_owned()));
        if let Some(marks) = marks {
            node.insert("marks".into(), marks);
        }
        top.content.push(Value::Object(node));
    }
}

fn link_allowed(href: &str) -> bool {
    let lower = href.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

fn code_block_node(language: Option<&str>, text: &str) -> Value {
    let mut node = Map::new();
    node.insert("type".into(), Value::String("codeBlock".into()));
    if let Some(language) = language.filter(|l| !l.is_empty()) {
        node.insert("attrs".into(), json!({"language": language}));
    }
    if !text.is_empty() {
        node.insert("content".into(), json!([{"type": "text", "text": text}]));
    }
    Value::Object(node)
}

pub(crate) fn markdown_to_adf(
    markdown: &str,
    code_block: Option<&JiraCodeBlock>,
) -> Result<Value, JiraError> {
    let options = Options::ENABLE_STRIKETHROUGH;
    let mut b = Builder {
        stack: vec![Open {
            kind: "doc",
            attrs: None,
            content: Vec::new(),
            implicit: false,
        }],
        marks: Vec::new(),
        code: None,
    };
    let mut events = Parser::new_ext(markdown, options).into_offset_iter();
    while let Some((event, range)) = events.next() {
        match event {
            Event::Start(Tag::Paragraph) => {
                b.close_implicit();
                b.open("paragraph", None);
            }
            Event::End(TagEnd::Paragraph) => b.close(),
            Event::Start(Tag::Heading { level, .. }) => {
                b.close_implicit();
                b.open("heading", Some(json!({"level": level as u8})));
            }
            Event::End(TagEnd::Heading(_)) => b.close(),
            Event::Start(Tag::BlockQuote(_)) => {
                b.close_implicit();
                b.open("blockquote", None);
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                b.close_implicit();
                b.close();
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                b.close_implicit();
                let language = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang = lang.split_whitespace().next().unwrap_or("").to_owned();
                        (!lang.is_empty()).then_some(lang)
                    }
                    CodeBlockKind::Indented => None,
                };
                b.code = Some((language, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, mut text)) = b.code.take() {
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    b.push_node(code_block_node(language.as_deref(), &text));
                }
            }
            Event::Start(Tag::List(start)) => {
                b.close_implicit();
                match start {
                    None => b.open("bulletList", None),
                    Some(1) => b.open("orderedList", None),
                    Some(n) => b.open("orderedList", Some(json!({"order": n}))),
                }
            }
            Event::End(TagEnd::List(_)) => b.close(),
            Event::Start(Tag::Item) => {
                b.close_implicit();
                b.open("listItem", None);
            }
            Event::End(TagEnd::Item) => {
                b.close_implicit();
                b.close();
            }
            Event::Start(Tag::Emphasis) => b.marks.push(Mark::Em),
            Event::Start(Tag::Strong) => b.marks.push(Mark::Strong),
            Event::Start(Tag::Strikethrough) => b.marks.push(Mark::Strike),
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) => {
                b.marks.pop();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                // A scheme outside https, http and mailto leaves the text plain (an empty href
                // is skipped when marks are written); the end event still pops one mark.
                let href = if link_allowed(&dest_url) {
                    dest_url.to_string()
                } else {
                    String::new()
                };
                b.marks.push(Mark::Link(href));
            }
            Event::End(TagEnd::Link) => {
                b.marks.pop();
            }
            Event::Start(Tag::Image { .. }) => {
                // An image becomes its source text; the events up to the matching end are skipped.
                let source = markdown.get(range).unwrap_or("").to_owned();
                for (inner, _) in events.by_ref() {
                    if matches!(inner, Event::End(TagEnd::Image)) {
                        break;
                    }
                }
                b.text(&source, None);
            }
            Event::Start(Tag::HtmlBlock) => {
                b.close_implicit();
                b.open("paragraph", None);
            }
            Event::End(TagEnd::HtmlBlock) => {
                // The block's last line ends in a line break that is not content.
                if let Some(top) = b.stack.last_mut() {
                    if let Some(Value::Object(last)) = top.content.last_mut() {
                        if let Some(Value::String(text)) = last.get_mut("text") {
                            while text.ends_with('\n') {
                                text.pop();
                            }
                        }
                    }
                }
                b.close();
            }
            Event::Text(text) => {
                if let Some((_, buffer)) = b.code.as_mut() {
                    buffer.push_str(&text);
                } else {
                    b.text(&text, None);
                }
            }
            Event::Code(text) => {
                b.text(&text, Some(json!({"type": "code"})));
            }
            Event::Html(text) | Event::InlineHtml(text) => b.text(&text, None),
            Event::SoftBreak | Event::HardBreak => {
                b.ensure_inline_parent();
                b.push_node(json!({"type": "hardBreak"}));
            }
            Event::Rule => {
                b.close_implicit();
                b.push_node(json!({"type": "rule"}));
            }
            // Everything else is not enabled in `Options`; its text arrives as `Text`.
            _ => {}
        }
    }
    while b.stack.len() > 1 {
        b.close();
    }
    let mut doc = b.stack.pop().map(|o| o.content).unwrap_or_default();
    if let Some(block) = code_block {
        doc.push(code_block_node(block.language.as_deref(), &block.text));
    }
    if doc.is_empty() {
        doc.push(json!({"type": "paragraph"}));
    }
    Ok(json!({"version": 1, "type": "doc", "content": doc}))
}

// ---------------------------------------------------------------- ADF to Markdown

struct Reader {
    code_blocks: Vec<JiraCodeBlock>,
}

fn kind_of(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

fn children(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn attr<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get("attrs")?.get(name)?.as_str()
}

/// All the text under a node, nothing else.
fn plain_text(node: &Value, depth: usize) -> String {
    if depth > MAX_DEPTH {
        return String::new();
    }
    if kind_of(node) == "text" {
        return node
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
    }
    if kind_of(node) == "hardBreak" {
        return "\n".to_owned();
    }
    children(node)
        .iter()
        .map(|c| plain_text(c, depth + 1))
        .collect()
}

/// Wraps `text` in `mark` keeping surrounding spaces outside, so the Markdown stays valid.
fn wrap(text: &str, mark: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_owned();
    }
    let start = &text[..text.len() - text.trim_start().len()];
    let end = &text[text.trim_end().len()..];
    format!("{start}{mark}{trimmed}{mark}{end}")
}

fn indent(text: &str, first: &str, rest: &str) -> String {
    text.lines()
        .enumerate()
        .map(|(i, line)| {
            let prefix = if i == 0 { first } else { rest };
            if line.is_empty() {
                prefix.trim_end().to_owned()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Reader {
    fn inline(&mut self, nodes: &[Value], depth: usize) -> String {
        nodes.iter().map(|n| self.inline_node(n, depth)).collect()
    }

    fn inline_node(&mut self, node: &Value, depth: usize) -> String {
        match kind_of(node) {
            "text" => {
                let mut out = node
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let marks = node
                    .get("marks")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let has = |name: &str| marks.iter().any(|m| kind_of(m) == name);
                if has("code") {
                    out = wrap(&out, "`");
                }
                if has("strike") {
                    out = wrap(&out, "~~");
                }
                if has("em") {
                    out = wrap(&out, "*");
                }
                if has("strong") {
                    out = wrap(&out, "**");
                }
                if let Some(href) = marks
                    .iter()
                    .find(|m| kind_of(m) == "link")
                    .and_then(|m| attr(m, "href"))
                {
                    out = format!("[{out}]({href})");
                }
                out
            }
            "hardBreak" => "  \n".to_owned(),
            "mention" => {
                let text = attr(node, "text").unwrap_or("");
                format!("@{}", text.trim_start_matches('@'))
            }
            "emoji" => attr(node, "text")
                .or_else(|| attr(node, "shortName"))
                .unwrap_or("")
                .to_owned(),
            "inlineCard" => format!("<{}>", attr(node, "url").unwrap_or("")),
            "status" => format!("[{}]", attr(node, "text").unwrap_or("")),
            "date" => date_text(node),
            "media" => media_text(node),
            _ => self.fallback(node, depth),
        }
    }

    /// The text content of an unknown node, or the placeholder when it has none.
    fn fallback(&mut self, node: &Value, depth: usize) -> String {
        let text = plain_text(node, depth);
        if text.is_empty() {
            format!("[Jira content ordin can't show: {}]", kind_of(node))
        } else {
            text
        }
    }

    fn blocks(&mut self, nodes: &[Value], separator: &str, depth: usize) -> String {
        nodes
            .iter()
            .map(|n| self.block(n, depth))
            .collect::<Vec<_>>()
            .join(separator)
    }

    fn block(&mut self, node: &Value, depth: usize) -> String {
        if depth > MAX_DEPTH {
            return plain_text(node, depth);
        }
        match kind_of(node) {
            "paragraph" => self.inline(children(node), depth + 1),
            "heading" => {
                let level = node
                    .get("attrs")
                    .and_then(|a| a.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(1, 6) as usize;
                format!(
                    "{} {}",
                    "#".repeat(level),
                    self.inline(children(node), depth + 1)
                )
            }
            "bulletList" => children(node)
                .iter()
                .map(|item| {
                    let body = self.blocks(children(item), "\n", depth + 1);
                    indent(&body, "- ", "  ")
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "orderedList" => {
                let start = node
                    .get("attrs")
                    .and_then(|a| a.get("order"))
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                children(node)
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        let marker = format!("{}. ", start + i as u64);
                        let rest = " ".repeat(marker.len());
                        let body = self.blocks(children(item), "\n", depth + 1);
                        indent(&body, &marker, &rest)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "codeBlock" => {
                let text = plain_text(node, depth);
                let language = attr(node, "language")
                    .filter(|l| !l.is_empty())
                    .map(str::to_owned);
                self.code_blocks.push(JiraCodeBlock {
                    language: language.clone(),
                    text: text.clone(),
                });
                // A fence longer than any run of backticks in the text keeps the block closed.
                let longest = text
                    .split(|c| c != '`')
                    .map(str::len)
                    .max()
                    .unwrap_or(0)
                    .max(2);
                let fence = "`".repeat(longest + 1);
                format!(
                    "{fence}{}\n{text}\n{fence}",
                    language.as_deref().unwrap_or("")
                )
            }
            "blockquote" | "panel" => {
                let body = self.blocks(children(node), "\n\n", depth + 1);
                indent(&body, "> ", "> ")
            }
            "rule" => "---".to_owned(),
            "table" => self.table(node, depth),
            "mediaSingle" | "mediaGroup" => self.blocks(children(node), "\n", depth + 1),
            "blockCard" => format!("<{}>", attr(node, "url").unwrap_or("")),
            "media" | "text" | "hardBreak" | "mention" | "emoji" | "inlineCard" | "status"
            | "date" => self.inline_node(node, depth + 1),
            _ => self.fallback(node, depth),
        }
    }

    fn table(&mut self, node: &Value, depth: usize) -> String {
        let rows: Vec<Vec<String>> = children(node)
            .iter()
            .map(|row| {
                children(row)
                    .iter()
                    .map(|cell| {
                        plain_text(cell, depth + 1)
                            .replace('\n', " ")
                            .replace('|', "\\|")
                    })
                    .collect()
            })
            .collect();
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return String::new();
        }
        let line = |row: &Vec<String>| {
            let mut cells = row.clone();
            cells.resize(width, String::new());
            format!("| {} |", cells.join(" | "))
        };
        let mut out = vec![line(&rows[0])];
        out.push(format!("|{}|", vec![" --- "; width].join("|")));
        out.extend(rows.iter().skip(1).map(line));
        out.join("\n")
    }
}

fn media_text(node: &Value) -> String {
    let name = attr(node, "alt").or_else(|| attr(node, "id")).unwrap_or("");
    format!("[attachment: {name}]")
}

/// `attrs.timestamp` (Unix milliseconds, as text) as `YYYY-MM-DD`.
fn date_text(node: &Value) -> String {
    let millis = node
        .get("attrs")
        .and_then(|a| a.get("timestamp"))
        .and_then(|t| {
            t.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| t.as_i64())
        });
    millis
        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

pub(crate) fn adf_to_markdown(adf: &Value) -> Result<(String, Vec<JiraCodeBlock>), JiraError> {
    if kind_of(adf) != "doc" {
        return Err(JiraError::ResponseInvalid);
    }
    let mut reader = Reader {
        code_blocks: Vec::new(),
    };
    let markdown = reader.blocks(children(adf), "\n\n", 0);
    Ok((markdown, reader.code_blocks))
}
