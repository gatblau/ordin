//! The Word document (DSK-DBX-1): the dashboard's name as title, the as-of line, then one section per
//! part — its heading and a real table whose first row repeats on each page. A knowledge page
//! (DSK-KEX-1) is written with the same package and styles.

use super::ooxml::{app_props, content_types, core_props, esc, root_rels, Package, XML_HEAD};
use super::page::{Block, Page, Run};
use super::Sheet;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// A4 less two 2.54 cm margins, in twentieths of a point.
const TEXT_WIDTH: usize = 9026;

pub(super) fn write(sheet: &Sheet) -> Vec<u8> {
    package(&sheet.title, &document(sheet), None)
}

/// A knowledge page as a Word document (DSK-KEX-1): the title, then its headings, paragraphs, lists,
/// tables and code, the lists numbered by `word/numbering.xml`.
pub(super) fn write_page(page: &Page) -> Vec<u8> {
    package(&page.title, &page_document(page), Some(numbering(page)))
}

/// The package: the document, its styles and, when given, its list numbering.
fn package(title: &str, document: &str, numbering: Option<String>) -> Vec<u8> {
    let mut package = Package::new();
    let mut overrides = vec![
        (
            "/word/document.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        ),
        (
            "/word/styles.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
        ),
    ];
    let mut rels = String::from(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    );
    if numbering.is_some() {
        overrides.push((
            "/word/numbering.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
        ));
        rels.push_str(
            r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
        );
    }
    package.add("[Content_Types].xml", content_types(&overrides).as_bytes());
    package.add(
        "_rels/.rels",
        root_rels("officeDocument", "word/document.xml").as_bytes(),
    );
    package.add("docProps/core.xml", core_props(title).as_bytes());
    package.add("docProps/app.xml", app_props().as_bytes());
    package.add(
        "word/_rels/document.xml.rels",
        format!(
            r#"{XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
        )
        .as_bytes(),
    );
    package.add("word/styles.xml", styles().as_bytes());
    if let Some(numbering) = numbering {
        package.add("word/numbering.xml", numbering.as_bytes());
    }
    package.add("word/document.xml", document.as_bytes());
    package.finish()
}

fn paragraph(style: &str, text: &str) -> String {
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        esc(text)
    )
}

fn table_cell(width: usize, text: &str, bold: bool) -> String {
    let run = if text.is_empty() {
        String::new()
    } else {
        format!(
            r#"<w:r>{}<w:t xml:space="preserve">{}</w:t></w:r>"#,
            if bold { "<w:rPr><w:b/></w:rPr>" } else { "" },
            esc(text)
        )
    };
    format!(r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/></w:tcPr><w:p>{run}</w:p></w:tc>"#)
}

fn document(sheet: &Sheet) -> String {
    let mut body = String::new();
    body.push_str(&paragraph("Title", &sheet.title));
    body.push_str(&paragraph("Subtitle", &sheet.subtitle));
    for part in &sheet.parts {
        body.push_str(&paragraph("Heading1", &part.heading));
        let columns = part.columns.len().max(1);
        let width = TEXT_WIDTH / columns;
        body.push_str(&format!(
            r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="{}" w:type="dxa"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>"#,
            width * columns
        ));
        for _ in 0..columns {
            body.push_str(&format!(r#"<w:gridCol w:w="{width}"/>"#));
        }
        body.push_str("</w:tblGrid>");
        body.push_str("<w:tr><w:trPr><w:tblHeader/></w:trPr>");
        if part.columns.is_empty() {
            body.push_str(&table_cell(width, "", true));
        }
        for name in &part.columns {
            body.push_str(&table_cell(width, name, true));
        }
        body.push_str("</w:tr>");
        for row in &part.rows {
            body.push_str("<w:tr>");
            for c in 0..columns {
                let text = row.get(c).map_or("", |cell| cell.shown());
                body.push_str(&table_cell(width, text, false));
            }
            body.push_str("</w:tr>");
        }
        body.push_str("</w:tbl><w:p/>");
    }
    wrap_body(&body)
}

/// The document part around `body`: A4 portrait with 2.54 cm margins.
fn wrap_body(body: &str) -> String {
    format!(
        r#"{XML_HEAD}<w:document xmlns:w="{W_NS}"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    )
}

/// The runs of one paragraph: bold, italic and code kept, a line break as `<w:br/>`.
fn runs_xml(runs: &[Run], force_bold: bool) -> String {
    let mut out = String::new();
    for run in runs {
        let mut props = String::new();
        if run.code {
            props.push_str(
                r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/>"#,
            );
        }
        if run.bold || force_bold {
            props.push_str("<w:b/>");
        }
        if run.italic {
            props.push_str("<w:i/>");
        }
        let props = if props.is_empty() {
            props
        } else {
            format!("<w:rPr>{props}</w:rPr>")
        };
        out.push_str(&format!("<w:r>{props}"));
        for (i, line) in run.text.split('\n').enumerate() {
            if i > 0 {
                out.push_str("<w:br/>");
            }
            if !line.is_empty() {
                out.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(line)));
            }
        }
        out.push_str("</w:r>");
    }
    out
}

fn styled(style: &str, runs: &[Run]) -> String {
    format!(
        r#"<w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr>{}</w:p>"#,
        runs_xml(runs, false)
    )
}

fn page_table(rows: &[Vec<Vec<Run>>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let width = TEXT_WIDTH / columns;
    let mut out = format!(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="{}" w:type="dxa"/><w:tblLook w:val="04A0" w:firstRow="1" w:lastRow="0" w:firstColumn="0" w:lastColumn="0" w:noHBand="0" w:noVBand="1"/></w:tblPr><w:tblGrid>"#,
        width * columns
    );
    for _ in 0..columns {
        out.push_str(&format!(r#"<w:gridCol w:w="{width}"/>"#));
    }
    out.push_str("</w:tblGrid>");
    for (r, row) in rows.iter().enumerate() {
        out.push_str(if r == 0 {
            "<w:tr><w:trPr><w:tblHeader/></w:trPr>"
        } else {
            "<w:tr>"
        });
        for c in 0..columns {
            let runs = row.get(c).map_or(&[][..], Vec::as_slice);
            out.push_str(&format!(
                r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/></w:tcPr><w:p>{}</w:p></w:tc>"#,
                runs_xml(runs, r == 0)
            ));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl><w:p/>");
    out
}

fn page_document(page: &Page) -> String {
    let mut body = paragraph("Title", &page.title);
    for block in &page.blocks {
        match block {
            Block::Heading { level, runs } => {
                body.push_str(&styled(&format!("Heading{level}"), runs));
            }
            Block::Paragraph(runs) => body.push_str(&styled("Normal", runs)),
            Block::Item {
                list, depth, runs, ..
            } => body.push_str(&format!(
                r#"<w:p><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="{depth}"/><w:numId w:val="{}"/></w:numPr></w:pPr>{}</w:p>"#,
                list + 1,
                runs_xml(runs, false)
            )),
            Block::Code(code) => {
                for line in code.split('\n') {
                    body.push_str(&paragraph("Code", line));
                }
            }
            Block::Table(rows) => body.push_str(&page_table(rows)),
            Block::Rule => body.push_str(
                r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="BFBFBF"/></w:pBdr></w:pPr></w:p>"#,
            ),
        }
    }
    wrap_body(&body)
}

/// `word/numbering.xml`: a bulleted and a numbered list definition, nine levels each, and one
/// numbering instance per list of the page, so each numbered list starts at its own first number.
fn numbering(page: &Page) -> String {
    let levels = |ordered: bool| -> String {
        (0..9)
            .map(|level| {
                let indent = 720 * (level + 1);
                let (format, text) = if ordered {
                    ("decimal", format!("%{}.", level + 1))
                } else {
                    ("bullet", ["\u{2022}", "\u{25E6}", "\u{25AA}"][level % 3].to_owned())
                };
                format!(
                    r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="{format}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{indent}" w:hanging="360"/></w:pPr></w:lvl>"#
                )
            })
            .collect()
    };
    let mut out = format!(
        r#"{XML_HEAD}<w:numbering xmlns:w="{W_NS}"><w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>{}</w:abstractNum><w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>{}</w:abstractNum>"#,
        levels(false),
        levels(true)
    );
    // The depth each list is drawn at, from its first item.
    for (id, (ordered, start)) in page.lists.iter().enumerate() {
        let depth = page
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Item { list, depth, .. } if *list == id => Some(*depth),
                _ => None,
            })
            .unwrap_or(0);
        let restart = if *ordered {
            format!(
                r#"<w:lvlOverride w:ilvl="{depth}"><w:startOverride w:val="{start}"/></w:lvlOverride>"#
            )
        } else {
            String::new()
        };
        out.push_str(&format!(
            r#"<w:num w:numId="{}"><w:abstractNumId w:val="{}"/>{restart}</w:num>"#,
            id + 1,
            u8::from(*ordered)
        ));
    }
    out.push_str("</w:numbering>");
    out
}

fn styles() -> String {
    let border =
        |side: &str| format!(r#"<w:{side} w:val="single" w:sz="4" w:space="0" w:color="BFBFBF"/>"#);
    let borders: String = ["top", "left", "bottom", "right", "insideH", "insideV"]
        .iter()
        .map(|side| border(side))
        .collect();
    format!(
        r#"{XML_HEAD}<w:styles xmlns:w="{W_NS}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-GB"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="80"/></w:pPr><w:rPr><w:sz w:val="52"/><w:szCs w:val="52"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Subtitle"><w:name w:val="Subtitle"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="240"/></w:pPr><w:rPr><w:color w:val="595959"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="30"/><w:szCs w:val="30"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="200" w:after="80"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="160" w:after="60"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="23"/><w:szCs w:val="23"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="40"/><w:ind w:left="720"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:qFormat/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F2"/></w:pPr><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New"/><w:sz w:val="19"/><w:szCs w:val="19"/></w:rPr></w:style><w:style w:type="table" w:default="1" w:styleId="TableNormal"><w:name w:val="Normal Table"/><w:tblPr><w:tblInd w:w="0" w:type="dxa"/><w:tblCellMar><w:top w:w="0" w:type="dxa"/><w:left w:w="108" w:type="dxa"/><w:bottom w:w="0" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style><w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:basedOn w:val="TableNormal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:tblPr><w:tblBorders>{borders}</w:tblBorders></w:tblPr></w:style></w:styles>"#
    )
}
