//! Markdown to Confluence storage format and back (ConfluenceAdapter.md Internal Logic 9). Both are pure.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use quick_xml::events::{BytesStart, Event as XmlEvent};
use quick_xml::Reader;

use crate::error::ConfluenceError;
use crate::push::MAX_PAGE_BODY_BYTES;
use crate::ConfluenceAdapter;

/// How deep storage XHTML may nest; deeper is treated as malformed, so a hostile page cannot exhaust the
/// stack of the recursive renderer.
const MAX_XML_DEPTH: usize = 256;

/// All text is XML-escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// The `info` macro of a copy's line (Internal Logic 7.2).
pub(crate) fn banner_macro(banner: &str) -> String {
    format!(
        "<ac:structured-macro ac:name=\"info\"><ac:rich-text-body><p>{}</p></ac:rich-text-body></ac:structured-macro>",
        escape(banner)
    )
}

fn heading_tag(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "h1",
        HeadingLevel::H2 => "h2",
        HeadingLevel::H3 => "h3",
        HeadingLevel::H4 => "h4",
        HeadingLevel::H5 => "h5",
        HeadingLevel::H6 => "h6",
    }
}

fn link_allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https:", "http:", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn is_relative_path(url: &str) -> bool {
    !url.is_empty() && !url.contains(':') && !url.starts_with('/') && !url.starts_with('#')
}

/// A `]]>` inside code is split across two CDATA sections.
fn cdata(code: &str) -> String {
    format!("<![CDATA[{}]]>", code.replace("]]>", "]]]]><![CDATA[>"))
}

/// The open code block: language and the text so far.
struct CodeBlock {
    language: String,
    code: String,
}

/// Internal Logic 9.1.
// spec: ordin-desktop/components/ConfluenceAdapter#ADR-028
fn to_storage(markdown: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let events: Vec<Event<'_>> = Parser::new_ext(markdown, options).collect();
    // Each open list: whether it is written as a Confluence task list (every item ticked or not).
    let mut lists: Vec<bool> = Vec::new();
    // Confluence numbers the tasks of a page; new ones are numbered from 1 in page order.
    let mut task_id = 0u32;
    let mut out = String::with_capacity(markdown.len() * 2);
    let mut code: Option<CodeBlock> = None;
    // Whether each open link was written as `<a>`: a link to another scheme becomes its text.
    let mut links: Vec<bool> = Vec::new();
    // Nesting inside an `![alt](…)`: its alt text is not page text.
    let mut image_depth = 0usize;
    let mut in_head = false;

    for (index, event) in events.iter().cloned().enumerate() {
        if image_depth > 0 {
            match event {
                Event::Start(Tag::Image { .. }) => image_depth += 1,
                Event::End(TagEnd::Image) => image_depth -= 1,
                _ => {}
            }
            continue;
        }
        if let Some(block) = code.as_mut() {
            match event {
                Event::End(TagEnd::CodeBlock) => {
                    out.push_str("<ac:structured-macro ac:name=\"code\">");
                    if !block.language.is_empty() {
                        out.push_str("<ac:parameter ac:name=\"language\">");
                        out.push_str(&escape(&block.language));
                        out.push_str("</ac:parameter>");
                    }
                    out.push_str("<ac:plain-text-body>");
                    out.push_str(&cdata(&block.code));
                    out.push_str("</ac:plain-text-body></ac:structured-macro>");
                    code = None;
                }
                Event::Text(text) => block.code.push_str(&text),
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => out.push_str("<p>"),
                Tag::Heading { level, .. } => {
                    out.push('<');
                    out.push_str(heading_tag(level));
                    out.push('>');
                }
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .unwrap_or_default()
                            .to_owned(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    code = Some(CodeBlock {
                        language,
                        code: String::new(),
                    });
                }
                // ADR-028: no markup passes through; it is shown as text.
                Tag::HtmlBlock => out.push_str("<p>"),
                Tag::List(Some(_)) => {
                    lists.push(false);
                    out.push_str("<ol>");
                }
                Tag::List(None) if is_task_list(&events, index) => {
                    lists.push(true);
                    out.push_str("<ac:task-list>");
                }
                Tag::List(None) => {
                    lists.push(false);
                    out.push_str("<ul>");
                }
                Tag::Item if lists.last() == Some(&true) => {
                    task_id += 1;
                    let status = if task_marker(&events, index) == Some(true) {
                        "complete"
                    } else {
                        "incomplete"
                    };
                    out.push_str(&format!(
                        "<ac:task><ac:task-id>{task_id}</ac:task-id><ac:task-status>{status}</ac:task-status><ac:task-body>"
                    ));
                }
                Tag::Item => out.push_str("<li>"),
                Tag::Table(_) => out.push_str("<table><tbody>"),
                Tag::TableHead => {
                    in_head = true;
                    out.push_str("<tr>");
                }
                Tag::TableRow => out.push_str("<tr>"),
                Tag::TableCell => out.push_str(if in_head { "<th>" } else { "<td>" }),
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong>"),
                Tag::Strikethrough => out.push_str("<del>"),
                Tag::Link { dest_url, .. } => {
                    let allowed = link_allowed(&dest_url);
                    links.push(allowed);
                    if allowed {
                        out.push_str("<a href=\"");
                        out.push_str(&escape(&dest_url));
                        out.push_str("\">");
                    }
                }
                Tag::Image { dest_url, .. } => {
                    image_depth = 1;
                    if is_relative_path(&dest_url) {
                        let name = dest_url.rsplit('/').next().unwrap_or(&dest_url);
                        out.push_str("<ac:image><ri:attachment ri:filename=\"");
                        out.push_str(&escape(name));
                        out.push_str("\"/></ac:image>");
                    }
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::HtmlBlock => out.push_str("</p>"),
                TagEnd::Heading(level) => {
                    out.push_str("</");
                    out.push_str(heading_tag(level));
                    out.push('>');
                }
                TagEnd::BlockQuote(_) => out.push_str("</blockquote>"),
                TagEnd::List(_) if lists.pop() == Some(true) => out.push_str("</ac:task-list>"),
                TagEnd::List(true) => out.push_str("</ol>"),
                TagEnd::List(false) => out.push_str("</ul>"),
                TagEnd::Item if lists.last() == Some(&true) => {
                    out.push_str("</ac:task-body></ac:task>");
                }
                TagEnd::Item => out.push_str("</li>"),
                TagEnd::Table => out.push_str("</tbody></table>"),
                TagEnd::TableHead => {
                    in_head = false;
                    out.push_str("</tr>");
                }
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => out.push_str(if in_head { "</th>" } else { "</td>" }),
                TagEnd::Emphasis => out.push_str("</em>"),
                TagEnd::Strong => out.push_str("</strong>"),
                TagEnd::Strikethrough => out.push_str("</del>"),
                TagEnd::Link if links.pop().unwrap_or(false) => out.push_str("</a>"),
                _ => {}
            },
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                out.push_str(&escape(&text));
            }
            Event::Code(text) => {
                out.push_str("<code>");
                out.push_str(&escape(&text));
                out.push_str("</code>");
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("<br/>"),
            Event::Rule => out.push_str("<hr/>"),
            // A tick box outside a task list stays visible as text.
            Event::TaskListMarker(done) if lists.last() != Some(&true) => {
                out.push_str(if done { "[x] " } else { "[ ] " });
            }
            _ => {}
        }
    }
    out
}

/// The tick box of the item starting at `item`, if it has one: right after the item, or after the
/// paragraph that opens a loose item.
fn task_marker(events: &[Event<'_>], item: usize) -> Option<bool> {
    match (events.get(item + 1), events.get(item + 2)) {
        (Some(Event::TaskListMarker(done)), _) => Some(*done),
        (Some(Event::Start(Tag::Paragraph)), Some(Event::TaskListMarker(done))) => Some(*done),
        _ => None,
    }
}

/// Whether the bulleted list starting at `list` is a task list: every one of its own items has a tick box.
fn is_task_list(events: &[Event<'_>], list: usize) -> bool {
    let mut depth = 0usize;
    let mut items = 0usize;
    for (index, event) in events.iter().enumerate().skip(list + 1) {
        match event {
            Event::Start(Tag::List(_)) => depth += 1,
            Event::End(TagEnd::List(_)) if depth == 0 => break,
            Event::End(TagEnd::List(_)) => depth -= 1,
            Event::Start(Tag::Item) if depth == 0 => {
                if task_marker(events, index).is_none() {
                    return false;
                }
                items += 1;
            }
            _ => {}
        }
    }
    items > 0
}

/// One element of storage XHTML, or a run of text.
#[derive(Debug)]
enum Node {
    Text(String),
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<Node>,
    },
}

fn text_of(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Element { children, .. } => out.push_str(&text_of(children)),
        }
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn element(start: &BytesStart<'_>) -> Result<Node, ConfluenceError> {
    let name = start.name().as_ref().to_owned();
    let mut attrs = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| ConfluenceError::ConversionFailed)?;
        let key = attribute.key.as_ref().to_owned();
        let raw = attribute.value.into_owned();
        let value = quick_xml::escape::unescape(&raw)
            .map(|v| v.into_owned())
            .unwrap_or(raw);
        attrs.push((key, value));
    }
    Ok(Node::Element {
        name,
        attrs,
        children: Vec::new(),
    })
}

fn push_node(stack: &mut [Node], top: &mut Vec<Node>, node: Node) {
    match stack.last_mut() {
        Some(Node::Element { children, .. }) => children.push(node),
        _ => top.push(node),
    }
}

fn push_text(stack: &mut [Node], top: &mut Vec<Node>, text: &str) {
    if !text.is_empty() {
        push_node(stack, top, Node::Text(text.to_owned()));
    }
}

/// A reference other than the five XML ones: `&nbsp;` is a plain space, the rest stay as written.
fn reference(name: &str) -> String {
    match name {
        "amp" => "&".to_owned(),
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "quot" => "\"".to_owned(),
        "apos" => "'".to_owned(),
        "nbsp" => " ".to_owned(),
        other => format!("&{other};"),
    }
}

/// Reads storage XHTML into a tree. An unclosed or mismatched element is `CONF_CONVERSION_FAILED`.
fn parse(xhtml: &str) -> Result<Vec<Node>, ConfluenceError> {
    let mut reader = Reader::from_str(xhtml);
    let mut top: Vec<Node> = Vec::new();
    let mut stack: Vec<Node> = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|_| ConfluenceError::ConversionFailed)?
        {
            XmlEvent::Start(start) => {
                if stack.len() >= MAX_XML_DEPTH {
                    return Err(ConfluenceError::ConversionFailed);
                }
                stack.push(element(&start)?);
            }
            XmlEvent::Empty(start) => {
                let node = element(&start)?;
                push_node(&mut stack, &mut top, node);
            }
            XmlEvent::End(_) => {
                let node = stack.pop().ok_or(ConfluenceError::ConversionFailed)?;
                push_node(&mut stack, &mut top, node);
            }
            XmlEvent::Text(text) => {
                push_text(&mut stack, &mut top, &text.xml10_content());
            }
            XmlEvent::CData(text) => {
                push_text(&mut stack, &mut top, &text.xml10_content());
            }
            XmlEvent::GeneralRef(entity) => {
                let text = match entity
                    .resolve_char_ref()
                    .map_err(|_| ConfluenceError::ConversionFailed)?
                {
                    Some(c) => c.to_string(),
                    None => reference(&entity.xml10_content()),
                };
                push_text(&mut stack, &mut top, &text);
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    if stack.is_empty() {
        Ok(top)
    } else {
        Err(ConfluenceError::ConversionFailed)
    }
}

/// Internal Logic 7.4: the storage body parses as XML.
pub(crate) fn check_xml(xhtml: &str) -> Result<(), ConfluenceError> {
    parse(xhtml).map(|_| ())
}

fn cannot_bring_back(name: &str) -> String {
    format!("[Confluence content ordin can't bring back: {name}]")
}

/// Text carried into Markdown is data: the characters Markdown would read as markup are escaped.
fn markdown_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn inline(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(text) => out.push_str(&markdown_text(text)),
            Node::Element {
                name,
                attrs,
                children,
            } => match name.as_str() {
                "em" => {
                    out.push('*');
                    out.push_str(&inline(children));
                    out.push('*');
                }
                "strong" => {
                    out.push_str("**");
                    out.push_str(&inline(children));
                    out.push_str("**");
                }
                "del" => {
                    out.push_str("~~");
                    out.push_str(&inline(children));
                    out.push_str("~~");
                }
                "code" => {
                    out.push('`');
                    out.push_str(&text_of(children));
                    out.push('`');
                }
                "a" => {
                    out.push('[');
                    out.push_str(&inline(children));
                    out.push_str("](");
                    out.push_str(attr(attrs, "href").unwrap_or_default());
                    out.push(')');
                }
                "br" => out.push_str("  \n"),
                "ac:image" => out.push_str(&image(children)),
                "ac:structured-macro" => {
                    out.push_str(&cannot_bring_back(
                        attr(attrs, "ac:name").unwrap_or("ac:structured-macro"),
                    ));
                }
                other => out.push_str(&cannot_bring_back(other)),
            },
        }
    }
    out
}

fn image(children: &[Node]) -> String {
    let file = children.iter().find_map(|node| match node {
        Node::Element { name, attrs, .. } if name == "ri:attachment" => attr(attrs, "ri:filename"),
        _ => None,
    });
    match file {
        Some(file) => format!("![{file}]({file})"),
        None => cannot_bring_back("ac:image"),
    }
}

fn code_macro(children: &[Node]) -> String {
    let mut language = String::new();
    let mut code = String::new();
    for node in children {
        if let Node::Element {
            name,
            attrs,
            children,
        } = node
        {
            match name.as_str() {
                "ac:parameter" if attr(attrs, "ac:name") == Some("language") => {
                    language = text_of(children);
                }
                "ac:plain-text-body" => code = text_of(children),
                _ => {}
            }
        }
    }
    let code = code.trim_end_matches('\n');
    format!("```{language}\n{code}\n```")
}

fn cell_text(node: &Node) -> String {
    match node {
        Node::Element { children, .. } => inline(children).replace('|', "\\|").replace('\n', " "),
        Node::Text(text) => markdown_text(text),
    }
}

fn table(children: &[Node]) -> String {
    let mut rows: Vec<&Node> = Vec::new();
    for node in children {
        if let Node::Element { name, children, .. } = node {
            match name.as_str() {
                "tbody" => rows.extend(
                    children
                        .iter()
                        .filter(|n| matches!(n, Node::Element { name, .. } if name == "tr")),
                ),
                "tr" => rows.push(node),
                _ => {}
            }
        }
    }
    let mut lines: Vec<String> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let Node::Element { children, .. } = row else {
            continue;
        };
        let cells: Vec<String> = children
            .iter()
            .filter(|n| matches!(n, Node::Element { .. }))
            .map(cell_text)
            .collect();
        lines.push(format!("| {} |", cells.join(" | ")));
        if index == 0 {
            lines.push(format!("|{}", " --- |".repeat(cells.len())));
        }
    }
    lines.join("\n")
}

fn indent(text: &str, by: &str) -> String {
    text.lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 || line.is_empty() {
                line.to_owned()
            } else {
                format!("{by}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn list(children: &[Node], ordered: bool) -> String {
    let mut lines = Vec::new();
    let mut number = 0;
    for node in children {
        if let Node::Element { name, children, .. } = node {
            if name != "li" {
                continue;
            }
            number += 1;
            let marker = if ordered {
                format!("{number}. ")
            } else {
                "- ".to_owned()
            };
            let body = blocks(children);
            lines.push(format!(
                "{marker}{}",
                indent(&body, &" ".repeat(marker.len()))
            ));
        }
    }
    lines.join("\n")
}

/// A Confluence task list as Markdown tick boxes: `- [x]` for a complete task, `- [ ]` otherwise.
fn task_list(children: &[Node]) -> String {
    let mut lines = Vec::new();
    for node in children {
        let Node::Element { name, children, .. } = node else {
            continue;
        };
        if name != "ac:task" {
            continue;
        }
        let mut done = false;
        let mut body = String::new();
        for part in children {
            if let Node::Element { name, children, .. } = part {
                match name.as_str() {
                    "ac:task-status" => done = text_of(children).trim() == "complete",
                    "ac:task-body" => body = blocks(children),
                    _ => {}
                }
            }
        }
        let marker = if done { "- [x] " } else { "- [ ] " };
        lines.push(format!("{marker}{}", indent(&body, "  ")).trim_end().to_owned());
    }
    lines.join("\n")
}

/// One block of Markdown per block element; loose text and inline elements gather into a paragraph.
fn blocks(nodes: &[Node]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut run: Vec<&Node> = Vec::new();

    fn flush(run: &mut Vec<&Node>, parts: &mut Vec<String>) {
        if run.is_empty() {
            return;
        }
        let owned: Vec<Node> = run.drain(..).map(copy_node).collect();
        let text = inline(&owned);
        if !text.trim().is_empty() {
            parts.push(text.trim().to_owned());
        }
    }

    for node in nodes {
        let block = match node {
            Node::Text(_) => None,
            Node::Element {
                name,
                attrs,
                children,
            } => match name.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    let level: usize = name[1..].parse().unwrap_or(1);
                    Some(format!("{} {}", "#".repeat(level), inline(children)))
                }
                "p" => Some(inline(children)),
                "ul" => Some(list(children, false)),
                "ac:task-list" => Some(task_list(children)),
                "ol" => Some(list(children, true)),
                "table" => Some(table(children)),
                "blockquote" => Some(
                    blocks(children)
                        .lines()
                        .map(|line| format!("> {line}").trim_end().to_owned())
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                "hr" => Some("---".to_owned()),
                "ac:structured-macro" if attr(attrs, "ac:name") == Some("code") => {
                    Some(code_macro(children))
                }
                "em" | "strong" | "del" | "code" | "a" | "br" | "ac:image" => None,
                "ac:structured-macro" => Some(cannot_bring_back(
                    attr(attrs, "ac:name").unwrap_or("ac:structured-macro"),
                )),
                other => Some(cannot_bring_back(other)),
            },
        };
        match block {
            Some(block) => {
                flush(&mut run, &mut parts);
                parts.push(block);
            }
            None => run.push(node),
        }
    }
    flush(&mut run, &mut parts);
    parts.join("\n\n")
}

fn copy_node(node: &Node) -> Node {
    match node {
        Node::Text(text) => Node::Text(text.clone()),
        Node::Element {
            name,
            attrs,
            children,
        } => Node::Element {
            name: name.clone(),
            attrs: attrs.clone(),
            children: children.iter().map(copy_node).collect(),
        },
    }
}

impl ConfluenceAdapter {
    /// CommonMark plus tables and strikethrough, as storage format; above `MAX_PAGE_BODY_BYTES` is
    /// `CONF_TOO_LARGE`.
    // spec: ordin-desktop/components/ConfluenceAdapter#ADR-048
    pub fn markdown_to_storage(markdown: &str) -> Result<String, ConfluenceError> {
        let storage = to_storage(markdown);
        if storage.len() > MAX_PAGE_BODY_BYTES {
            return Err(ConfluenceError::TooLarge);
        }
        Ok(storage)
    }

    /// The same element set in reverse, for **Bring it in**. Anything else becomes a line naming it.
    pub fn storage_to_markdown(storage_xhtml: &str) -> Result<String, ConfluenceError> {
        let nodes = parse(storage_xhtml)?;
        let mut markdown = blocks(&nodes);
        if !markdown.is_empty() {
            markdown.push('\n');
        }
        Ok(markdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage(markdown: &str) -> String {
        ConfluenceAdapter::markdown_to_storage(markdown).unwrap()
    }

    fn markdown(xhtml: &str) -> String {
        ConfluenceAdapter::storage_to_markdown(xhtml).unwrap()
    }

    #[test]
    fn the_spec_example_converts_exactly() {
        assert_eq!(
            storage("# Rates\n\nDay rate: **£640**\n"),
            "<h1>Rates</h1><p>Day rate: <strong>£640</strong></p>"
        );
    }

    #[test]
    fn inline_elements_convert() {
        assert_eq!(
            storage("*a* **b** ~~c~~ `d`"),
            "<p><em>a</em> <strong>b</strong> <del>c</del> <code>d</code></p>"
        );
    }

    #[test]
    fn every_heading_level_converts() {
        assert_eq!(
            storage("## a\n\n### b\n\n#### c\n\n##### d\n\n###### e"),
            "<h2>a</h2><h3>b</h3><h4>c</h4><h5>d</h5><h6>e</h6>"
        );
    }

    #[test]
    fn a_fenced_block_is_a_code_macro() {
        assert_eq!(
            storage("```rust\nlet a = 1;\n```\n"),
            "<ac:structured-macro ac:name=\"code\"><ac:parameter ac:name=\"language\">rust</ac:parameter>\
             <ac:plain-text-body><![CDATA[let a = 1;\n]]></ac:plain-text-body></ac:structured-macro>"
        );
    }

    #[test]
    fn an_indented_block_has_no_language() {
        let out = storage("    plain\n");
        assert!(out.contains("<![CDATA[plain\n]]>"), "{out}");
        assert!(!out.contains("ac:parameter"), "{out}");
    }

    #[test]
    fn a_cdata_end_inside_code_is_split() {
        let out = storage("```\na]]>b\n```\n");
        assert!(out.contains("<![CDATA[a]]]]><![CDATA[>b\n]]>"), "{out}");
        assert!(markdown(&out).contains("a]]>b"));
    }

    #[test]
    fn lists_quotes_and_rules_convert() {
        assert_eq!(
            storage("- a\n- b\n\n1. c\n\n> d\n\n---\n"),
            "<ul><li>a</li><li>b</li></ul><ol><li>c</li></ol><blockquote><p>d</p></blockquote><hr/>"
        );
    }

    #[test]
    fn a_task_list_is_read_as_tick_boxes() {
        let page = "<ac:task-list><ac:task><ac:task-id>1</ac:task-id><ac:task-status>complete</ac:task-status>\
                    <ac:task-body>Send the <strong>rates</strong></ac:task-body></ac:task>\
                    <ac:task><ac:task-id>2</ac:task-id><ac:task-status>incomplete</ac:task-status>\
                    <ac:task-body><p>Sign</p></ac:task-body></ac:task></ac:task-list><p>after</p>";
        assert_eq!(markdown(page), "- [x] Send the **rates**\n- [ ] Sign\n\nafter\n");
    }

    #[test]
    fn tick_boxes_are_written_as_a_task_list_and_read_back() {
        let out = storage("- [x] a\n- [ ] b\n");
        assert_eq!(
            out,
            "<ac:task-list><ac:task><ac:task-id>1</ac:task-id><ac:task-status>complete</ac:task-status>\
             <ac:task-body>a</ac:task-body></ac:task><ac:task><ac:task-id>2</ac:task-id>\
             <ac:task-status>incomplete</ac:task-status><ac:task-body>b</ac:task-body></ac:task></ac:task-list>"
        );
        assert_eq!(markdown(&out), "- [x] a\n- [ ] b\n");
    }

    #[test]
    fn a_list_with_some_tick_boxes_stays_a_list_and_keeps_them_as_text() {
        let out = storage("- [x] a\n- b\n\n1. c\n\n- [ ] d\n");
        assert_eq!(
            out,
            "<ul><li>[x] a</li><li>b</li></ul><ol><li>c</li></ol><ac:task-list><ac:task><ac:task-id>1</ac:task-id>\
             <ac:task-status>incomplete</ac:task-status><ac:task-body>d</ac:task-body></ac:task></ac:task-list>"
        );
    }

    #[test]
    fn tables_use_th_then_td() {
        assert_eq!(
            storage("| a | b |\n|---|---|\n| 1 | 2 |\n"),
            "<table><tbody><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></tbody></table>"
        );
    }

    #[test]
    fn only_three_schemes_become_links() {
        assert_eq!(
            storage("[a](https://acme.example/x) [b](mailto:a@acme.example)"),
            "<p><a href=\"https://acme.example/x\">a</a> <a href=\"mailto:a@acme.example\">b</a></p>"
        );
        assert_eq!(storage("[a](javascript:alert)"), "<p>a</p>");
        assert_eq!(storage("[a](other.md)"), "<p>a</p>");
    }

    #[test]
    fn a_relative_image_is_an_attachment_reference() {
        assert_eq!(
            storage("![chart](images/chart.png)"),
            "<p><ac:image><ri:attachment ri:filename=\"chart.png\"/></ac:image></p>"
        );
        assert_eq!(storage("![far](https://acme.example/a.png)"), "<p></p>");
    }

    #[test]
    fn raw_html_is_shown_as_text() {
        assert_eq!(
            storage("<script>alert(1)</script>"),
            "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>"
        );
        assert_eq!(storage("a <b>c</b>"), "<p>a &lt;b&gt;c&lt;/b&gt;</p>");
    }

    #[test]
    fn hard_breaks_and_escapes() {
        assert_eq!(storage("a  \nb & c"), "<p>a<br/>b &amp; c</p>");
    }

    #[test]
    fn a_body_above_the_limit_is_too_large() {
        let huge = "x".repeat(MAX_PAGE_BODY_BYTES + 1);
        assert_eq!(
            ConfluenceAdapter::markdown_to_storage(&huge)
                .unwrap_err()
                .code(),
            "CONF_TOO_LARGE"
        );
    }

    #[test]
    fn the_spec_example_converts_back() {
        assert_eq!(
            markdown("<h1>Rates</h1><p>Day rate: <strong>£640</strong></p>"),
            "# Rates\n\nDay rate: **£640**\n"
        );
    }

    #[test]
    fn the_element_set_comes_back() {
        let source = "## Head\n\n*a* ~~b~~ `c` [d](https://acme.example/)\n\n- one\n- two\n\n1. three\n\n> quoted\n\n---\n\n```rust\nlet a = 1;\n```\n\n| x | y |\n| --- | --- |\n| 1 | 2 |\n\n![pic.png](pic.png)\n";
        assert_eq!(markdown(&storage(source)), source);
    }

    #[test]
    fn nested_lists_keep_their_indent() {
        let out = markdown("<ul><li><p>a</p><ul><li>b</li></ul></li></ul>");
        assert_eq!(out, "- a\n\n  - b\n");
    }

    #[test]
    fn other_elements_and_macros_are_named() {
        assert_eq!(
            markdown("<ac:structured-macro ac:name=\"jira\"><ac:parameter ac:name=\"key\">X</ac:parameter></ac:structured-macro><div>a</div>"),
            "[Confluence content ordin can't bring back: jira]\n\n[Confluence content ordin can't bring back: div]\n"
        );
        assert_eq!(
            markdown("<p>a <span>b</span> <ac:structured-macro ac:name=\"status\"/></p>"),
            "a [Confluence content ordin can't bring back: span] [Confluence content ordin can't bring back: status]\n"
        );
        assert_eq!(
            markdown("<ac:image><ri:url ri:value=\"x\"/></ac:image>"),
            "[Confluence content ordin can't bring back: ac:image]\n"
        );
    }

    #[test]
    fn entities_and_markup_characters_are_data() {
        assert_eq!(
            markdown("<p>a &amp; b&nbsp;c &#163;1 *d*</p>"),
            "a & b c £1 \\*d\\*\n"
        );
        assert_eq!(markdown("<p>&unknown;</p>"), "&unknown;\n");
    }

    #[test]
    fn loose_text_gathers_into_a_paragraph() {
        assert_eq!(markdown("plain <strong>bold</strong>"), "plain **bold**\n");
        assert_eq!(markdown(""), "");
        assert_eq!(markdown("<p>a<br/>b</p>"), "a  \nb\n");
    }

    #[test]
    fn malformed_xml_fails() {
        for bad in ["<p>unclosed", "<p></q>", "</p>", "<p a=1>x</p>"] {
            assert_eq!(
                ConfluenceAdapter::storage_to_markdown(bad)
                    .unwrap_err()
                    .code(),
                "CONF_CONVERSION_FAILED",
                "{bad}"
            );
        }
        assert!(check_xml("<p>ok</p>").is_ok());
    }

    #[test]
    fn nesting_past_the_limit_is_refused() {
        let deep = "<p>".repeat(MAX_XML_DEPTH + 1);
        assert_eq!(
            check_xml(&deep).unwrap_err().code(),
            "CONF_CONVERSION_FAILED"
        );
    }
}
