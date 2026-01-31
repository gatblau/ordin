//! The PDF (DSK-DBX-1): A4 portrait pages with the dashboard's name, the as-of line, then each part's
//! heading and its table, the column names in bold and repeated at the top of each page a table goes
//! on to. Written with Helvetica and Helvetica-Bold, two of the standard fonts every PDF reader has,
//! in WinAnsi encoding; a character outside it is shown as `?`. A cell wider than its column is cut
//! and ends with `...`.
//!
//! A knowledge page (DSK-KEX-1) is written on the same pages: its title, headings, paragraphs with
//! bold, italic (Helvetica-Oblique) and code (Courier), lists with their bullets or numbers, tables
//! and code blocks.

use super::page::{plain, Block, Page, Run};
use super::{Part, Sheet};

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 50.0;
const TITLE_SIZE: f32 = 20.0;
const SUBTITLE_SIZE: f32 = 10.0;
const HEADING_SIZE: f32 = 14.0;
const CELL_SIZE: f32 = 9.0;
const ROW_H: f32 = 15.0;
/// An estimate of Helvetica's mean character width, as a fraction of the font size.
const CHAR_W: f32 = 0.55;

/// The five standard fonts the files use, as `/F1` to `/F5` on every page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Font {
    Regular,
    Bold,
    Mono,
    Italic,
    BoldItalic,
}

impl Font {
    const ALL: [(Font, &'static str); 5] = [
        (Font::Regular, "Helvetica"),
        (Font::Bold, "Helvetica-Bold"),
        (Font::Mono, "Courier"),
        (Font::Italic, "Helvetica-Oblique"),
        (Font::BoldItalic, "Helvetica-BoldOblique"),
    ];

    fn name(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Mono => "F3",
            Font::Italic => "F4",
            Font::BoldItalic => "F5",
        }
    }

    /// An estimate of the font's mean character width, as a fraction of the size.
    fn char_w(self) -> f32 {
        if self == Font::Mono {
            0.6
        } else {
            CHAR_W
        }
    }

    fn of(run: &Run, force_bold: bool) -> Font {
        match (run.code, run.bold || force_bold, run.italic) {
            (true, _, _) => Font::Mono,
            (false, true, true) => Font::BoldItalic,
            (false, true, false) => Font::Bold,
            (false, false, true) => Font::Italic,
            (false, false, false) => Font::Regular,
        }
    }
}

/// One page's content stream being written.
struct Pages {
    done: Vec<String>,
    current: String,
    y: f32,
}

impl Pages {
    fn new() -> Pages {
        Pages {
            done: Vec::new(),
            current: String::new(),
            y: PAGE_H - MARGIN,
        }
    }

    fn new_page(&mut self) {
        self.done.push(std::mem::take(&mut self.current));
        self.y = PAGE_H - MARGIN;
    }

    /// Starts a new page unless `height` still fits on this one.
    fn room_for(&mut self, height: f32) {
        if self.y - height < MARGIN {
            self.new_page();
        }
    }

    fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, text: &str) {
        self.text_in(
            if bold { Font::Bold } else { Font::Regular },
            x,
            y,
            size,
            text,
        );
    }

    fn text_in(&mut self, font: Font, x: f32, y: f32, size: f32, text: &str) {
        let font = font.name();
        self.current.push_str(&format!(
            "BT /{font} {size:.1} Tf {x:.2} {y:.2} Td ({}) Tj ET\n",
            literal(text)
        ));
    }

    fn line(&mut self, x1: f32, y: f32, x2: f32) {
        self.current.push_str(&format!(
            "0.75 G 0.5 w {x1:.2} {y:.2} m {x2:.2} {y:.2} l S 0 G\n"
        ));
    }

    fn finish(mut self) -> Vec<String> {
        self.done.push(self.current);
        self.done
    }
}

/// `text` wrapped to lines of at most `width` points at `size`.
fn wrap(text: &str, size: f32, width: f32) -> Vec<String> {
    let max = ((width / (size * CHAR_W)) as usize).max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let word: String = word.chars().take(max).collect();
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > max {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// `text` cut to fit `width` points at `size`, ending with `...` when cut.
fn fit(text: &str, size: f32, width: f32) -> String {
    let max = ((width / (size * CHAR_W)) as usize).max(1);
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(3)).collect();
    format!("{}...", kept.trim_end())
}

/// A PDF literal string's content: `\`, `(` and `)` escaped, each character in WinAnsi.
fn literal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => match win_ansi(c) {
                Some(code) => out.push_str(&format!("\\{code:03o}")),
                None => out.push('?'),
            },
        }
    }
    out
}

/// The WinAnsi code of a character beyond ASCII, when it has one.
fn win_ansi(c: char) -> Option<u8> {
    let code = c as u32;
    if (0xA0..=0xFF).contains(&code) {
        return u8::try_from(code).ok();
    }
    Some(match c {
        '€' => 0x80,
        '‚' => 0x82,
        '„' => 0x84,
        '…' => 0x85,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '™' => 0x99,
        '\t' | '\n' | '\r' => b' ',
        _ => return None,
    })
}

fn draw_header(pages: &mut Pages, part: &Part, col_w: f32) {
    let y = pages.y - CELL_SIZE;
    for (c, name) in part.columns.iter().enumerate() {
        let x = MARGIN + c as f32 * col_w;
        pages.text(x, y, CELL_SIZE, true, &fit(name, CELL_SIZE, col_w - 4.0));
    }
    pages.y -= ROW_H;
    pages.line(MARGIN, pages.y + 3.0, PAGE_W - MARGIN);
}

pub(super) fn write(sheet: &Sheet) -> Vec<u8> {
    let width = PAGE_W - 2.0 * MARGIN;
    let mut pages = Pages::new();
    for line in wrap(&sheet.title, TITLE_SIZE, width) {
        pages.room_for(TITLE_SIZE * 1.3);
        pages.y -= TITLE_SIZE;
        let y = pages.y;
        pages.text(MARGIN, y, TITLE_SIZE, true, &line);
        pages.y -= TITLE_SIZE * 0.3;
    }
    pages.y -= SUBTITLE_SIZE * 1.2;
    let y = pages.y;
    pages.text(
        MARGIN,
        y,
        SUBTITLE_SIZE,
        false,
        &fit(&sheet.subtitle, SUBTITLE_SIZE, width),
    );
    pages.y -= SUBTITLE_SIZE;
    for part in &sheet.parts {
        // The heading stays with its column names and first row.
        pages.y -= HEADING_SIZE;
        pages.room_for(HEADING_SIZE * 1.5 + 2.0 * ROW_H);
        for line in wrap(&part.heading, HEADING_SIZE, width) {
            pages.y -= HEADING_SIZE;
            let y = pages.y;
            pages.text(MARGIN, y, HEADING_SIZE, true, &line);
            pages.y -= HEADING_SIZE * 0.3;
        }
        pages.y -= 6.0;
        let col_w = width / part.columns.len().max(1) as f32;
        draw_header(&mut pages, part, col_w);
        for row in &part.rows {
            if pages.y - ROW_H < MARGIN {
                pages.new_page();
                draw_header(&mut pages, part, col_w);
            }
            let y = pages.y - CELL_SIZE;
            for (c, cell) in row.iter().enumerate() {
                let x = MARGIN + c as f32 * col_w;
                pages.text(
                    x,
                    y,
                    CELL_SIZE,
                    false,
                    &fit(cell.shown(), CELL_SIZE, col_w - 4.0),
                );
            }
            pages.y -= ROW_H;
            pages.line(MARGIN, pages.y + 3.0, PAGE_W - MARGIN);
        }
    }
    assemble(&pages.finish())
}

const BODY_SIZE: f32 = 10.5;
const CODE_SIZE: f32 = 9.0;

/// A word, or the part of a word, in one font.
struct Piece {
    text: String,
    font: Font,
    space_before: bool,
}

/// `runs` cut into words, each in its run's font; `None` is a line break.
fn pieces(runs: &[Run], force_bold: bool) -> Vec<Option<Piece>> {
    let mut out = Vec::new();
    let mut space = false;
    for run in runs {
        let font = Font::of(run, force_bold);
        let mut current: Option<Piece> = None;
        for c in run.text.chars() {
            if c == '\n' {
                out.extend(current.take().map(Some));
                out.push(None);
                space = false;
            } else if c.is_whitespace() {
                out.extend(current.take().map(Some));
                space = true;
            } else if let Some(piece) = current.as_mut() {
                piece.text.push(c);
            } else {
                current = Some(Piece {
                    text: c.to_string(),
                    font,
                    space_before: space,
                });
                space = false;
            }
        }
        out.extend(current.take().map(Some));
    }
    out
}

/// One drawn line: each piece's offset from the line's start, its font and its text.
type Line = Vec<(f32, Font, String)>;

/// The pieces laid out on lines of at most `width` points at `size`; a word wider than a line is
/// cut over several.
fn flow_lines(pieces: Vec<Option<Piece>>, size: f32, width: f32) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line: Line = Vec::new();
    let mut x = 0.0_f32;
    for piece in pieces {
        let Some(piece) = piece else {
            lines.push(std::mem::take(&mut line));
            x = 0.0;
            continue;
        };
        let char_w = size * piece.font.char_w();
        let w = |t: &str| t.chars().count() as f32 * char_w;
        let mut gap = if piece.space_before && !line.is_empty() {
            size * 0.28
        } else {
            0.0
        };
        let mut text = piece.text;
        if !line.is_empty() && x + gap + w(&text) > width {
            lines.push(std::mem::take(&mut line));
            x = 0.0;
            gap = 0.0;
        }
        x += gap;
        loop {
            let max = (((width - x) / char_w) as usize).max(1);
            if text.chars().count() <= max {
                break;
            }
            let head: String = text.chars().take(max).collect();
            text = text.chars().skip(max).collect();
            line.push((x, piece.font, head));
            lines.push(std::mem::take(&mut line));
            x = 0.0;
        }
        let advance = w(&text);
        line.push((x, piece.font, text));
        x += advance;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// `runs` drawn as lines from `indent`, with `marker` (a bullet or a number) before the first line.
fn flow(
    pages: &mut Pages,
    runs: &[Run],
    size: f32,
    indent: f32,
    force_bold: bool,
    marker: Option<String>,
) {
    let width = PAGE_W - 2.0 * MARGIN - indent;
    let line_h = size * 1.4;
    for (i, line) in flow_lines(pieces(runs, force_bold), size, width)
        .into_iter()
        .enumerate()
    {
        pages.room_for(line_h);
        pages.y -= size;
        let y = pages.y;
        if i == 0 {
            if let Some(marker) = &marker {
                pages.text(MARGIN + indent - 14.0, y, size, false, marker);
            }
        }
        for (x, font, text) in line {
            pages.text_in(font, MARGIN + indent + x, y, size, &text);
        }
        pages.y -= line_h - size;
    }
}

/// One table row, each cell cut to its column.
fn page_row(pages: &mut Pages, cells: &[String], col_w: f32, bold: bool) {
    let y = pages.y - CELL_SIZE;
    for (c, text) in cells.iter().enumerate() {
        let x = MARGIN + c as f32 * col_w;
        pages.text(x, y, CELL_SIZE, bold, &fit(text, CELL_SIZE, col_w - 4.0));
    }
    pages.y -= ROW_H;
    pages.line(MARGIN, pages.y + 3.0, PAGE_W - MARGIN);
}

/// A knowledge page as a PDF (DSK-KEX-1).
pub(super) fn write_page(page: &Page) -> Vec<u8> {
    let width = PAGE_W - 2.0 * MARGIN;
    let mut pages = Pages::new();
    for line in wrap(&page.title, TITLE_SIZE, width) {
        pages.room_for(TITLE_SIZE * 1.3);
        pages.y -= TITLE_SIZE;
        let y = pages.y;
        pages.text(MARGIN, y, TITLE_SIZE, true, &line);
        pages.y -= TITLE_SIZE * 0.3;
    }
    pages.y -= 8.0;
    for block in &page.blocks {
        match block {
            Block::Heading { level, runs } => {
                let size = match level {
                    1 => 16.0,
                    2 => 13.5,
                    _ => 12.0,
                };
                pages.y -= size * 0.6;
                // A heading stays with the first line under it.
                pages.room_for(size * 1.4 + BODY_SIZE * 1.4);
                flow(&mut pages, runs, size, 0.0, true, None);
                pages.y -= 2.0;
            }
            Block::Paragraph(runs) => {
                flow(&mut pages, runs, BODY_SIZE, 0.0, false, None);
                pages.y -= BODY_SIZE * 0.5;
            }
            Block::Item {
                ordered,
                number,
                depth,
                runs,
                ..
            } => {
                let indent = 18.0 * f32::from(*depth + 1);
                let marker = if *ordered {
                    format!("{number}.")
                } else {
                    "\u{2022}".to_owned()
                };
                flow(&mut pages, runs, BODY_SIZE, indent, false, Some(marker));
                pages.y -= BODY_SIZE * 0.2;
            }
            Block::Code(code) => {
                let max = ((width - 8.0) / (CODE_SIZE * Font::Mono.char_w())) as usize;
                for line in code.split('\n') {
                    let chars: Vec<char> = line.chars().collect();
                    let chunks: Vec<String> = if chars.is_empty() {
                        vec![String::new()]
                    } else {
                        chars
                            .chunks(max.max(1))
                            .map(|c| c.iter().collect())
                            .collect()
                    };
                    for chunk in chunks {
                        pages.room_for(CODE_SIZE * 1.35);
                        pages.y -= CODE_SIZE;
                        let y = pages.y;
                        pages.text_in(Font::Mono, MARGIN + 8.0, y, CODE_SIZE, &chunk);
                        pages.y -= CODE_SIZE * 0.35;
                    }
                }
                pages.y -= 6.0;
            }
            Block::Table(rows) => {
                let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
                let col_w = width / columns as f32;
                let texts: Vec<Vec<String>> = rows
                    .iter()
                    .map(|row| row.iter().map(|cell| plain(cell)).collect())
                    .collect();
                let header = texts.first().cloned().unwrap_or_default();
                pages.y -= 4.0;
                pages.room_for(2.0 * ROW_H);
                page_row(&mut pages, &header, col_w, true);
                for row in texts.iter().skip(1) {
                    if pages.y - ROW_H < MARGIN {
                        pages.new_page();
                        page_row(&mut pages, &header, col_w, true);
                    }
                    page_row(&mut pages, row, col_w, false);
                }
                pages.y -= 8.0;
            }
            Block::Rule => {
                pages.room_for(12.0);
                pages.y -= 6.0;
                let y = pages.y;
                pages.line(MARGIN, y, PAGE_W - MARGIN);
                pages.y -= 6.0;
            }
        }
    }
    assemble(&pages.finish())
}

/// The PDF file: catalogue, page tree, the five fonts, then each page and its content.
fn assemble(contents: &[String]) -> Vec<u8> {
    let count = contents.len();
    let fonts = Font::ALL.len();
    // Objects: 1 catalogue, 2 page tree, 3 to 7 fonts, then a page and its content per page.
    let first_page = 3 + fonts;
    let kids: Vec<String> = (0..count)
        .map(|i| format!("{} 0 R", first_page + 2 * i))
        .collect();
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} >>",
            kids.join(" ")
        )
        .into_bytes(),
    ];
    let mut resources = String::new();
    for (i, (font, base)) in Font::ALL.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>"
            )
            .into_bytes(),
        );
        resources.push_str(&format!("/{} {} 0 R ", font.name(), 3 + i));
    }
    for (i, content) in contents.iter().enumerate() {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << {resources}>> >> /Contents {} 0 R >>",
                first_page + 1 + 2 * i
            )
            .into_bytes(),
        );
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(content.as_bytes());
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
pub(super) fn literal_for_test(text: &str) -> String {
    literal(text)
}
