//! The Excel workbook (DSK-DBX-1): one sheet per part, named after its heading, the column names in
//! bold on row 1 and one row per data row below. Numbers are stored as numbers; a percentage column
//! carries Excel's percentage format and a money or decimals column two or its own decimal places.

use std::collections::HashSet;

use super::ooxml::{app_props, content_types, core_props, esc, root_rels, Package, XML_HEAD};
use super::{Cell, NumberKind, Part, Sheet};

const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
/// Excel's longest sheet name.
const SHEET_NAME_MAX: usize = 31;

/// Style indexes of `styles.xml`'s `cellXfs`.
const STYLE_HEADER: u8 = 1;
const STYLE_PERCENT: u8 = 2;
const STYLE_TWO_PLACES: u8 = 3;
const STYLE_WHOLE: u8 = 4;

pub(super) fn write(sheet: &Sheet) -> Vec<u8> {
    // A report with no part still gives a workbook: one sheet holding the title and the as-of line.
    let fallback;
    let parts: &[Part] = if sheet.parts.is_empty() {
        fallback = [Part {
            heading: sheet.title.clone(),
            columns: vec![sheet.title.clone()],
            rows: vec![vec![Cell::Text(sheet.subtitle.clone())]],
        }];
        &fallback
    } else {
        &sheet.parts
    };
    let names = sheet_names(parts);

    let mut package = Package::new();
    let mut overrides = vec![
        (
            "/xl/workbook.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
        ),
        (
            "/xl/styles.xml".to_owned(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml",
        ),
    ];
    for i in 1..=parts.len() {
        overrides.push((
            format!("/xl/worksheets/sheet{i}.xml"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
        ));
    }
    package.add("[Content_Types].xml", content_types(&overrides).as_bytes());
    package.add(
        "_rels/.rels",
        root_rels("officeDocument", "xl/workbook.xml").as_bytes(),
    );
    package.add("docProps/core.xml", core_props(&sheet.title).as_bytes());
    package.add("docProps/app.xml", app_props().as_bytes());

    let mut workbook = format!(
        r#"{XML_HEAD}<workbook xmlns="{MAIN_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#
    );
    let mut rels = format!(
        r#"{XML_HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#
    );
    for (i, name) in names.iter().enumerate() {
        let n = i + 1;
        workbook.push_str(&format!(
            r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#,
            esc(name)
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#
        ));
    }
    workbook.push_str("</sheets></workbook>");
    rels.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
        parts.len() + 1
    ));
    package.add("xl/workbook.xml", workbook.as_bytes());
    package.add("xl/_rels/workbook.xml.rels", rels.as_bytes());
    package.add("xl/styles.xml", styles().as_bytes());
    for (i, part) in parts.iter().enumerate() {
        package.add(
            &format!("xl/worksheets/sheet{}.xml", i + 1),
            worksheet(part).as_bytes(),
        );
    }
    package.finish()
}

/// One unique sheet name per part: the heading without `[ ] : * ? / \`, without an apostrophe at
/// either end, cut to 31 characters; "Sheet N" when nothing is left; " (2)", " (3)"… added to a
/// name already used (Excel compares names without regard to case).
pub(super) fn sheet_names(parts: &[Part]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut names = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let cleaned: String = part
            .heading
            .chars()
            .filter(|c| !"[]:*?/\\".contains(*c) && !c.is_control())
            .collect();
        let cleaned = cleaned.trim().trim_matches('\'').trim().to_owned();
        let base = if cleaned.is_empty() {
            format!("Sheet {}", i + 1)
        } else {
            cut(&cleaned, SHEET_NAME_MAX)
        };
        let mut name = base.clone();
        let mut n = 2;
        while used.contains(&name.to_lowercase()) {
            let suffix = format!(" ({n})");
            name = format!(
                "{}{suffix}",
                cut(&base, SHEET_NAME_MAX - suffix.chars().count())
            );
            n += 1;
        }
        used.insert(name.to_lowercase());
        names.push(name);
    }
    names
}

fn cut(text: &str, max: usize) -> String {
    text.chars()
        .take(max)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// The column letters of the zero-based `index` (0 → A, 26 → AA).
pub(super) fn column_letters(mut index: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

fn worksheet(part: &Part) -> String {
    let mut out = format!(r#"{XML_HEAD}<worksheet xmlns="{MAIN_NS}">"#);
    if !part.columns.is_empty() {
        out.push_str(&format!(
            r#"<cols><col min="1" max="{}" width="18" customWidth="1"/></cols>"#,
            part.columns.len()
        ));
    }
    out.push_str("<sheetData>");
    out.push_str(r#"<row r="1">"#);
    for (c, name) in part.columns.iter().enumerate() {
        out.push_str(&text_cell(
            &format!("{}1", column_letters(c)),
            name,
            Some(STYLE_HEADER),
        ));
    }
    out.push_str("</row>");
    for (r, row) in part.rows.iter().enumerate() {
        let n = r + 2;
        out.push_str(&format!(r#"<row r="{n}">"#));
        for (c, cell) in row.iter().enumerate() {
            let at = format!("{}{n}", column_letters(c));
            match cell {
                Cell::Empty => {}
                Cell::Text(text) => out.push_str(&text_cell(&at, text, None)),
                Cell::Number { raw, kind, .. } => {
                    let style = match kind {
                        NumberKind::Plain => None,
                        NumberKind::Percentage => Some(STYLE_PERCENT),
                        NumberKind::Decimals(0) => Some(STYLE_WHOLE),
                        NumberKind::Decimals(_) => Some(STYLE_TWO_PLACES),
                    };
                    let style = style.map(|s| format!(r#" s="{s}""#)).unwrap_or_default();
                    out.push_str(&format!(r#"<c r="{at}"{style}><v>{}</v></c>"#, esc(raw)));
                }
            }
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData></worksheet>");
    out
}

fn text_cell(at: &str, text: &str, style: Option<u8>) -> String {
    let style = style.map(|s| format!(r#" s="{s}""#)).unwrap_or_default();
    format!(
        r#"<c r="{at}" t="inlineStr"{style}><is><t xml:space="preserve">{}</t></is></c>"#,
        esc(text)
    )
}

fn styles() -> String {
    format!(
        r#"{XML_HEAD}<styleSheet xmlns="{MAIN_NS}"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="5"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="4" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="3" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
    )
}
