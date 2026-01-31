//! A knowledge page laid out for a file (OrdinDesktop.md DSK-KEX-1; amended January 2026; user
//! decision; Approved by the user January 2026): its Markdown read with `pulldown-cmark`, the reader
//! the workspace already uses, into the blocks the PDF and Word writers draw. Headings become
//! levels 1 to 3 (deeper ones level 3), paragraphs keep bold, italic and code, lists keep their
//! kind and depth, tables stay tables, code blocks are monospace, and a link is its text followed
//! by its address in brackets. A page link (`[[Page]]`) is its text only: it names a page, not an
//! address. Raw HTML, images' pictures and front matter are left out; an image keeps its words.

use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};

/// A stretch of text in one style.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Run {
    /// The words; a `\n` is a line break inside the block.
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

/// One block of the page, in reading order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    /// `level` is 1, 2 or 3.
    Heading {
        level: u8,
        runs: Vec<Run>,
    },
    Paragraph(Vec<Run>),
    /// One list item. `list` numbers the list it belongs to across the page (each nested list is a
    /// list of its own); `number` is the item's number in an ordered list.
    Item {
        list: usize,
        ordered: bool,
        number: u64,
        depth: u8,
        runs: Vec<Run>,
    },
    /// A code block's lines, as written.
    Code(String),
    /// A table: the first row is its header row.
    Table(Vec<Vec<Vec<Run>>>),
    Rule,
}

/// The page ready to be written: its title and its blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Page {
    pub title: String,
    pub blocks: Vec<Block>,
    /// The kind of each list, by its `list` number: true for a numbered list, with its first number.
    pub lists: Vec<(bool, u64)>,
}

/// The item being read: its list, kind, number and depth.
struct OpenItem {
    list: usize,
    ordered: bool,
    number: u64,
    depth: u8,
}

#[derive(Default)]
struct Reader {
    blocks: Vec<Block>,
    lists: Vec<(bool, u64)>,
    runs: Vec<Run>,
    bold: u32,
    italic: u32,
    /// The open lists: their number, kind and next item number.
    list_stack: Vec<(usize, bool, u64)>,
    item: Option<OpenItem>,
    /// The open links: the address shown after the text, when there is one.
    links: Vec<Option<String>>,
    code: Option<String>,
    table: Option<Vec<Vec<Vec<Run>>>>,
    row: Vec<Vec<Run>>,
    in_metadata: bool,
}

impl Reader {
    fn push_text(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        let (bold, italic) = (self.bold > 0, self.italic > 0);
        if let Some(last) = self.runs.last_mut() {
            if last.bold == bold && last.italic == italic && last.code == code {
                last.text.push_str(text);
                return;
            }
        }
        self.runs.push(Run {
            text: text.to_owned(),
            bold,
            italic,
            code,
        });
    }

    /// The open item, with the words read so far, becomes a block.
    fn flush_item(&mut self) {
        if let Some(item) = self.item.take() {
            let runs = std::mem::take(&mut self.runs);
            self.blocks.push(Block::Item {
                list: item.list,
                ordered: item.ordered,
                number: item.number,
                depth: item.depth,
                runs,
            });
        }
    }

    /// Words read outside any paragraph (a tight list item's) that no block has taken.
    fn flush_loose(&mut self) {
        if self.item.is_some() {
            self.flush_item();
        } else if self.runs.iter().any(|r| !r.text.trim().is_empty()) {
            let runs = std::mem::take(&mut self.runs);
            self.blocks.push(Block::Paragraph(runs));
        } else {
            self.runs.clear();
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph | Tag::Heading { .. } => {
                if self.item.is_none() && self.table.is_none() {
                    self.flush_loose();
                }
            }
            Tag::BlockQuote(_) => {
                self.flush_loose();
                self.italic += 1;
            }
            Tag::CodeBlock(_) => {
                self.flush_loose();
                self.code = Some(String::new());
            }
            Tag::List(first) => {
                self.flush_loose();
                let id = self.lists.len();
                self.lists.push((first.is_some(), first.unwrap_or(1)));
                self.list_stack
                    .push((id, first.is_some(), first.unwrap_or(1)));
            }
            Tag::Item => {
                self.flush_loose();
                let depth = u8::try_from(self.list_stack.len().saturating_sub(1)).unwrap_or(8);
                if let Some((list, ordered, next)) = self.list_stack.last_mut() {
                    self.item = Some(OpenItem {
                        list: *list,
                        ordered: *ordered,
                        number: *next,
                        depth: depth.min(8),
                    });
                    *next += 1;
                }
            }
            Tag::Table(_) => {
                self.flush_loose();
                self.table = Some(Vec::new());
            }
            Tag::TableHead | Tag::TableRow => self.row.clear(),
            Tag::TableCell => self.runs.clear(),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let shown = match link_type {
                    LinkType::WikiLink { .. } | LinkType::Autolink | LinkType::Email => None,
                    _ if dest_url.trim().is_empty() || dest_url.starts_with('#') => None,
                    _ => Some(dest_url.to_string()),
                };
                self.links.push(shown);
            }
            Tag::Image { dest_url, .. } => {
                let url = dest_url.trim();
                self.links.push((!url.is_empty()).then(|| url.to_owned()));
            }
            Tag::MetadataBlock(_) => self.in_metadata = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if self.table.is_some() {
                    return;
                }
                if self.item.is_some() {
                    self.flush_item();
                } else if !self.runs.is_empty() {
                    let runs = std::mem::take(&mut self.runs);
                    self.blocks.push(Block::Paragraph(runs));
                }
            }
            TagEnd::Heading(level) => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    _ => 3,
                };
                let runs = std::mem::take(&mut self.runs);
                self.blocks.push(Block::Heading { level, runs });
            }
            TagEnd::BlockQuote(_) => {
                self.flush_loose();
                self.italic = self.italic.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.blocks
                        .push(Block::Code(code.trim_end_matches('\n').to_owned()));
                }
            }
            TagEnd::List(_) => {
                self.flush_loose();
                self.list_stack.pop();
            }
            TagEnd::Item => self.flush_loose(),
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.runs);
                self.row.push(cell);
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let row = std::mem::take(&mut self.row);
                if let Some(table) = self.table.as_mut() {
                    table.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.blocks.push(Block::Table(table));
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => {
                if let Some(Some(url)) = self.links.pop() {
                    let (bold, italic) = (self.bold, self.italic);
                    self.bold = 0;
                    self.italic = 0;
                    self.push_text(&format!(" ({url})"), false);
                    self.bold = bold;
                    self.italic = italic;
                }
            }
            TagEnd::MetadataBlock(_) => self.in_metadata = false,
            _ => {}
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            // Front matter is the page's details, not its words.
            Event::Text(_) if self.in_metadata => {}
            Event::Text(text) => {
                if let Some(code) = self.code.as_mut() {
                    code.push_str(&text);
                } else {
                    self.push_text(&text, false);
                }
            }
            Event::Code(text) => self.push_text(&text, true),
            Event::InlineMath(text) | Event::DisplayMath(text) => self.push_text(&text, false),
            Event::SoftBreak => self.push_text(" ", false),
            Event::HardBreak => self.push_text("\n", false),
            Event::Rule => {
                self.flush_loose();
                self.blocks.push(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                self.push_text(if done { "[x] " } else { "[ ] " }, false)
            }
            Event::FootnoteReference(name) => self.push_text(&format!("[{name}]"), false),
            Event::Html(_) | Event::InlineHtml(_) => {}
        }
    }
}

/// `markdown` read into blocks under `title`.
pub(crate) fn lay_out_page(title: &str, markdown: &str) -> Page {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_WIKILINKS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut reader = Reader::default();
    for event in Parser::new_ext(markdown, options) {
        reader.event(event);
    }
    reader.flush_loose();
    Page {
        title: title.to_owned(),
        blocks: reader.blocks,
        lists: reader.lists,
    }
}

/// A block's runs as plain words, for a table cell or a test.
pub(crate) fn plain(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}
