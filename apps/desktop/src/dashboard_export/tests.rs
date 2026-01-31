//! DSK-DBX-1 to DSK-DBX-3: each file a dashboard is saved as is opened and its structure checked —
//! the Office files unzipped and every XML part parsed, the PDF's header and pages read — and slides
//! added to a presentation leave its own slides as they were.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};

use ordin_service_api::commands::dashboards::DashboardExportFormat;
use ordin_types::domain::{
    CellValue, ChartKind, ColumnType, Report, ReportData, ReportKeyFigure, ReportSection,
    ReportTableColumn,
};
use ordin_types::num::DecimalString;
use ordin_types::time::Timestamp;
use ordin_types::{ReportId, RunId, WorkspaceId};

use super::pptx::{add_sheet, choose_layout, ROWS_PER_SLIDE};
use super::xlsx::{column_letters, sheet_names};
use super::*;

fn column(name: &str, column_type: ColumnType, currency: Option<&str>) -> ReportTableColumn {
    ReportTableColumn {
        name: name.to_owned(),
        column_type,
        currency: currency.map(str::to_owned),
        decimals: None,
    }
}

fn text(value: &str) -> CellValue {
    CellValue::Text(value.to_owned())
}

fn num(value: &str) -> CellValue {
    CellValue::Number(DecimalString(value.to_owned()))
}

/// A report of three parts: a table of `rows` rows, a chart and key figures.
fn report(rows: usize) -> Report {
    let table_rows = (0..rows)
        .map(|i| {
            vec![
                text(&format!("Supplier & Co {i}")),
                num("412.5"),
                num("0.08"),
                CellValue::Null,
            ]
        })
        .collect();
    Report {
        id: ReportId::new(),
        workspace_id: WorkspaceId::new(),
        run_id: RunId::new(),
        title: "Supplier prices".to_owned(),
        as_of: Timestamp::now(),
        parts: vec![
            ReportSection {
                heading: "Rates <by> supplier".to_owned(),
                data: ReportData::Table {
                    columns: vec![
                        column("Supplier", ColumnType::Text, None),
                        column("Rate", ColumnType::Money, Some("GBP")),
                        column("Change", ColumnType::Percentage, None),
                        column("Note", ColumnType::Text, None),
                    ],
                    rows: table_rows,
                },
                sources: vec![],
            },
            ReportSection {
                heading: "Spend by month".to_owned(),
                data: ReportData::Chart {
                    chart_kind: ChartKind::Line,
                    columns: vec![
                        column("Month", ColumnType::Text, None),
                        column("Spend", ColumnType::Number, None),
                    ],
                    rows: vec![
                        vec![text("January"), num("1200")],
                        vec![text("February"), num("1350")],
                    ],
                    x_column: "Month".to_owned(),
                    series_columns: vec!["Spend".to_owned()],
                    x_title: "Month".to_owned(),
                    y_title: "Spend".to_owned(),
                    bar_arrangement: None,
                },
                sources: vec![],
            },
            ReportSection {
                heading: "Headlines".to_owned(),
                data: ReportData::KeyFigures {
                    figures: vec![ReportKeyFigure {
                        label: "Suppliers".to_owned(),
                        value: DecimalString("14".to_owned()),
                        unit: String::new(),
                        change_against: None,
                    }],
                },
                sources: vec![],
            },
        ],
    }
}

fn words() -> Words {
    Words::english("As of 10 January 2026, 10:43".to_owned())
}

/// Every file of a zip package, by name.
fn unzip(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip package");
    let mut parts = BTreeMap::new();
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("an entry");
        let mut content = Vec::new();
        file.read_to_end(&mut content).expect("readable");
        parts.insert(file.name().expect("a name").into_owned(), content);
    }
    parts
}

/// Every `.xml` and `.rels` part parses as well-formed XML.
fn assert_well_formed(parts: &BTreeMap<String, Vec<u8>>) {
    for (name, bytes) in parts {
        if !(name.ends_with(".xml") || name.ends_with(".rels")) {
            continue;
        }
        let text = std::str::from_utf8(bytes).expect("UTF-8");
        let mut reader = quick_xml::Reader::from_str(text);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{name} is not well-formed: {e}"),
            }
        }
    }
}

fn part(parts: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(parts.get(name).unwrap_or_else(|| panic!("{name}")).clone()).expect("UTF-8")
}

/// Every relationship target of every `.rels` part names a part that exists.
fn assert_targets_exist(parts: &BTreeMap<String, Vec<u8>>) {
    for (name, bytes) in parts {
        let Some(owner) = name.strip_suffix(".rels") else {
            continue;
        };
        // `ppt/slides/_rels/slide1.xml.rels` belongs to `ppt/slides/slide1.xml`.
        let folder = owner
            .rsplit_once("_rels/")
            .map_or("", |(folder, _)| folder)
            .to_owned();
        let text = std::str::from_utf8(bytes).expect("UTF-8");
        for target in text
            .split("Target=\"")
            .skip(1)
            .filter_map(|t| t.split('"').next())
        {
            let mut path: Vec<&str> = folder.split('/').filter(|s| !s.is_empty()).collect();
            for segment in target.split('/') {
                match segment {
                    ".." => {
                        path.pop();
                    }
                    "." | "" => {}
                    s => path.push(s),
                }
            }
            let resolved = path.join("/");
            assert!(
                parts.contains_key(&resolved),
                "{name} → {target} ({resolved})"
            );
        }
    }
}

// ---- Laying the report out ----

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
#[test]
fn every_part_becomes_a_table() {
    let sheet = lay_out("Prices", &report(2), &words());
    assert_eq!(sheet.title, "Prices");
    assert_eq!(sheet.subtitle, "As of 10 January 2026, 10:43");
    assert_eq!(sheet.parts.len(), 3);
    let table = &sheet.parts[0];
    assert_eq!(table.columns, ["Supplier", "Rate", "Change", "Note"]);
    assert_eq!(table.rows[0][1].shown(), "412.50 GBP");
    assert_eq!(table.rows[0][2].shown(), "8%");
    assert_eq!(table.rows[0][3], Cell::Empty);
    // A chart is the table of the rows it is drawn from.
    assert_eq!(sheet.parts[1].columns, ["Month", "Spend"]);
    assert_eq!(sheet.parts[1].rows.len(), 2);
    // Key figures are one row per figure.
    assert_eq!(sheet.parts[2].columns, ["Figure", "Value", "Unit"]);
    assert_eq!(sheet.parts[2].rows[0][0].shown(), "Suppliers");
    assert_eq!(sheet.parts[2].rows[0][1].shown(), "14");
}

#[test]
fn file_names_hold_no_character_a_folder_refuses() {
    assert_eq!(
        file_name("Q3: spend/rates?", DashboardExportFormat::Pdf),
        "Q3  spend rates.pdf"
    );
    assert_eq!(
        file_name("  ", DashboardExportFormat::Xlsx),
        "Dashboard.xlsx"
    );
    assert_eq!(
        file_name("Prices", DashboardExportFormat::Pptx),
        "Prices.pptx"
    );
    assert_eq!(
        file_name("Prices", DashboardExportFormat::Docx),
        "Prices.docx"
    );
}

// ---- Excel ----

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
#[test]
fn excel_holds_a_sheet_per_part() {
    let bytes = render("Prices", &report(3), &words(), DashboardExportFormat::Xlsx);
    let parts = unzip(&bytes);
    assert_well_formed(&parts);
    assert_targets_exist(&parts);
    let workbook = part(&parts, "xl/workbook.xml");
    assert_eq!(workbook.matches("<sheet ").count(), 3);
    assert!(workbook.contains(r#"name="Rates &lt;by&gt; supplier""#));
    let types = part(&parts, "[Content_Types].xml");
    for n in 1..=3 {
        assert!(types.contains(&format!("/xl/worksheets/sheet{n}.xml")));
    }
    let first = part(&parts, "xl/worksheets/sheet1.xml");
    assert_eq!(
        first.matches("<row ").count(),
        4,
        "the column names and three rows"
    );
    assert!(first.contains("<t xml:space=\"preserve\">Supplier &amp; Co 0</t>"));
    // Numbers are numbers, the percentage in Excel's percentage format.
    assert!(first.contains(r#"<c r="B2" s="3"><v>412.5</v></c>"#));
    assert!(first.contains(r#"<c r="C2" s="2"><v>0.08</v></c>"#));
    // A missing value is an empty cell.
    assert!(!first.contains(r#"r="D2""#));
    let figures = part(&parts, "xl/worksheets/sheet3.xml");
    assert!(figures.contains(r#"<c r="B2"><v>14</v></c>"#));
}

#[test]
fn sheet_names_are_short_unique_and_allowed() {
    let parts: Vec<Part> = ["Rates", "rates", "a/b:c*d?[e]", "", "x".repeat(40).as_str()]
        .iter()
        .map(|heading| Part {
            heading: (*heading).to_owned(),
            columns: vec![],
            rows: vec![],
        })
        .collect();
    let names = sheet_names(&parts);
    assert_eq!(names[0], "Rates");
    assert_eq!(names[1], "rates (2)");
    assert_eq!(names[2], "abcde");
    assert_eq!(names[3], "Sheet 4");
    assert_eq!(names[4].chars().count(), 31);
    assert_eq!(column_letters(0), "A");
    assert_eq!(column_letters(25), "Z");
    assert_eq!(column_letters(26), "AA");
    assert_eq!(column_letters(701), "ZZ");
    assert_eq!(column_letters(702), "AAA");
}

#[test]
fn a_report_with_no_part_still_gives_a_workbook() {
    let mut empty = report(0);
    empty.parts.clear();
    let parts = unzip(&render(
        "Prices",
        &empty,
        &words(),
        DashboardExportFormat::Xlsx,
    ));
    assert_well_formed(&parts);
    assert_eq!(
        part(&parts, "xl/workbook.xml").matches("<sheet ").count(),
        1
    );
}

// ---- Word ----

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
#[test]
fn word_holds_a_title_then_a_section_and_table_per_part() {
    let bytes = render("Prices", &report(3), &words(), DashboardExportFormat::Docx);
    let parts = unzip(&bytes);
    assert_well_formed(&parts);
    assert_targets_exist(&parts);
    assert!(part(&parts, "[Content_Types].xml").contains("wordprocessingml.document.main+xml"));
    let document = part(&parts, "word/document.xml");
    assert!(document.contains(
        r#"<w:pStyle w:val="Title"/></w:pPr><w:r><w:t xml:space="preserve">Prices</w:t>"#
    ));
    assert_eq!(
        document.matches(r#"<w:pStyle w:val="Heading1"/>"#).count(),
        3
    );
    assert_eq!(document.matches("<w:tbl>").count(), 3);
    // The first table: the column names (repeated on each page) and three rows.
    let first = &document[document.find("<w:tbl>").unwrap()..document.find("</w:tbl>").unwrap()];
    assert_eq!(first.matches("<w:tr>").count(), 4);
    assert_eq!(first.matches("<w:tblHeader/>").count(), 1);
    assert!(first.contains("412.50 GBP"));
    let styles = part(&parts, "word/styles.xml");
    for style in ["Title", "Subtitle", "Heading1", "TableGrid"] {
        assert!(
            styles.contains(&format!(r#"w:styleId="{style}""#)),
            "{style}"
        );
    }
}

// ---- PowerPoint ----

/// The slide parts, by number.
fn slides_of(parts: &BTreeMap<String, Vec<u8>>) -> BTreeMap<u64, String> {
    parts
        .iter()
        .filter_map(|(name, bytes)| {
            let n = name
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((n, String::from_utf8(bytes.clone()).ok()?))
        })
        .collect()
}

/// The `id` and `r:id` of each `<p:sldId>`, in order.
fn slide_ids(presentation: &str) -> Vec<(u64, String)> {
    presentation
        .split("<p:sldId ")
        .skip(1)
        .map(|tag| {
            let id = tag
                .split("id=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let rid = tag
                .split("r:id=\"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            (id.parse().unwrap(), rid.to_owned())
        })
        .collect()
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
#[test]
fn powerpoint_holds_a_title_slide_then_a_slide_per_part() {
    let bytes = render("Prices", &report(3), &words(), DashboardExportFormat::Pptx);
    let parts = unzip(&bytes);
    assert_well_formed(&parts);
    assert_targets_exist(&parts);
    let slides = slides_of(&parts);
    assert_eq!(slides.len(), 4, "the title and three parts");
    assert!(slides[&1].contains("<a:t>Prices</a:t>"));
    assert!(slides[&1].contains("<a:t>As of 10 January 2026, 10:43</a:t>"));
    // Tables are real tables: the column names and three rows.
    let table = &slides[&2];
    assert_eq!(table.matches("<a:tbl>").count(), 1);
    assert_eq!(table.matches("<a:tr ").count(), 4);
    assert_eq!(table.matches("<a:gridCol ").count(), 4);
    assert!(table.contains("<a:t>Rates &lt;by&gt; supplier</a:t>"));
    let presentation = part(&parts, "ppt/presentation.xml");
    assert_eq!(slide_ids(&presentation).len(), 4);
    let types = part(&parts, "[Content_Types].xml");
    for n in 1..=4 {
        assert!(types.contains(&format!("/ppt/slides/slide{n}.xml")));
    }
}

#[test]
fn a_long_table_goes_on_over_several_slides() {
    let rows = ROWS_PER_SLIDE * 2 + 1;
    let parts = unzip(&render(
        "Prices",
        &report(rows),
        &words(),
        DashboardExportFormat::Pptx,
    ));
    let slides = slides_of(&parts);
    // The title, three slides of the long table, the chart and the key figures.
    assert_eq!(slides.len(), 6);
    assert_eq!(slides[&2].matches("<a:tr ").count(), ROWS_PER_SLIDE + 1);
    assert_eq!(
        slides[&4].matches("<a:tr ").count(),
        2,
        "the column names repeated, one row"
    );
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-2
#[test]
fn adding_to_a_presentation_keeps_its_slides_and_adds_the_new_ones() {
    let mut first = report(1);
    first.parts.truncate(1);
    let original = render(
        "Last quarter",
        &first,
        &words(),
        DashboardExportFormat::Pptx,
    );
    let before = unzip(&original);
    let before_ids = slide_ids(&part(&before, "ppt/presentation.xml"));
    assert_eq!(before_ids.len(), 2);

    let sheet = lay_out("Prices", &report(3), &words());
    let merged = add_sheet(&original, &sheet).expect("added");
    let after = unzip(&merged);
    assert_well_formed(&after);
    assert_targets_exist(&after);

    // Every part of the original is still there; only the three that list slides changed.
    for (name, bytes) in &before {
        let changed = [
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
            "[Content_Types].xml",
        ];
        if !changed.contains(&name.as_str()) {
            assert_eq!(after.get(name), Some(bytes), "{name} unchanged");
        }
    }
    let slides = slides_of(&after);
    assert_eq!(slides.len(), 6, "two kept and four added");
    assert!(
        slides[&3].contains("<a:t>Prices</a:t>"),
        "the new title slide follows the old ones"
    );

    // The original slides come first, in their order, with their ids.
    let ids = slide_ids(&part(&after, "ppt/presentation.xml"));
    assert_eq!(ids.len(), 6);
    assert_eq!(&ids[..2], &before_ids[..]);
    let unique: HashSet<u64> = ids.iter().map(|(id, _)| *id).collect();
    assert_eq!(unique.len(), 6);
    // Each new relationship id is unique and names a new slide.
    let rels = part(&after, "ppt/_rels/presentation.xml.rels");
    let rel_ids: Vec<&str> = rels
        .split(" Id=\"")
        .skip(1)
        .filter_map(|r| r.split('"').next())
        .collect();
    assert_eq!(rel_ids.len(), rel_ids.iter().collect::<HashSet<_>>().len());
    for (_, rid) in &ids[2..] {
        assert!(rel_ids.contains(&rid.as_str()), "{rid}");
    }
    let types = part(&after, "[Content_Types].xml");
    for n in 3..=6 {
        assert!(types.contains(&format!("/ppt/slides/slide{n}.xml")));
    }

    // Adding again goes on after them, with no id used twice.
    let again = add_sheet(&merged, &sheet).expect("added again");
    let again = unzip(&again);
    assert_targets_exist(&again);
    assert_eq!(slides_of(&again).len(), 10);
    let ids = slide_ids(&part(&again, "ppt/presentation.xml"));
    assert_eq!(
        ids.iter().map(|(id, _)| id).collect::<HashSet<_>>().len(),
        10
    );
    assert_eq!(ids.iter().map(|(_, r)| r).collect::<HashSet<_>>().len(), 10);
}

#[test]
fn added_slides_follow_the_presentations_size_and_blank_layout() {
    let original = render(
        "Last quarter",
        &report(1),
        &words(),
        DashboardExportFormat::Pptx,
    );
    let mut parts: Vec<(String, Vec<u8>)> = unzip(&original).into_iter().collect();
    // A 4:3 presentation.
    for (name, bytes) in &mut parts {
        if name == "ppt/presentation.xml" {
            let text = String::from_utf8(bytes.clone()).unwrap();
            *bytes = text
                .replace(
                    r#"cx="12192000" cy="6858000""#,
                    r#"cx="9144000" cy="6858000""#,
                )
                .into_bytes();
        }
    }
    let mut package = super::ooxml::Package::new();
    for (name, bytes) in &parts {
        package.add(name, bytes);
    }
    let merged =
        add_sheet(&package.finish(), &lay_out("Prices", &report(1), &words())).expect("added");
    let slides = slides_of(&unzip(&merged));
    // No shape is wider than the 4:3 slide.
    let last = slides.values().last().unwrap();
    for ext in last.split(r#"<a:ext cx=""#).skip(1) {
        let cx: u64 = ext.split('"').next().unwrap().parse().unwrap();
        assert!(cx <= 9_144_000, "{cx}");
    }

    // The blank layout is chosen over the first one.
    let layouts = vec![
        (
            "ppt/slideLayouts/slideLayout1.xml".to_owned(),
            br#"<p:sldLayout type="title">"#.to_vec(),
        ),
        (
            "ppt/slideLayouts/slideLayout7.xml".to_owned(),
            br#"<p:sldLayout type="blank">"#.to_vec(),
        ),
        (
            "ppt/slideLayouts/slideLayout2.xml".to_owned(),
            br#"<p:sldLayout type="obj">"#.to_vec(),
        ),
    ];
    assert_eq!(choose_layout(&layouts).as_deref(), Some("slideLayout7.xml"));
    assert_eq!(
        choose_layout(&layouts[..1]).as_deref(),
        Some("slideLayout1.xml")
    );
    assert_eq!(choose_layout(&[]), None);
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-2
#[test]
fn a_file_that_is_not_a_presentation_is_refused() {
    let sheet = lay_out("Prices", &report(1), &words());
    assert_eq!(
        add_sheet(b"not a zip", &sheet),
        Err(AddSlidesError::Unreadable)
    );
    let workbook = render("Prices", &report(1), &words(), DashboardExportFormat::Xlsx);
    assert_eq!(
        add_sheet(&workbook, &sheet),
        Err(AddSlidesError::Unreadable)
    );
    let document = render("Prices", &report(1), &words(), DashboardExportFormat::Docx);
    assert_eq!(
        add_sheet(&document, &sheet),
        Err(AddSlidesError::Unreadable)
    );
}

// ---- PDF ----

// spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
#[test]
fn the_pdf_is_a_paged_document() {
    let bytes = render("Prices", &report(3), &words(), DashboardExportFormat::Pdf);
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let document = lopdf::Document::load_mem(&bytes).expect("a PDF");
    assert_eq!(document.get_pages().len(), 1);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("(Prices) Tj"));
    assert!(text.contains("(Rates <by> supplier) Tj"));
    assert!(text.contains("(412.50 GBP) Tj"));
}

#[test]
fn a_long_table_goes_on_over_several_pages() {
    let bytes = render("Prices", &report(150), &words(), DashboardExportFormat::Pdf);
    let document = lopdf::Document::load_mem(&bytes).expect("a PDF");
    let pages = document.get_pages().len();
    assert!(pages >= 3, "{pages} pages");
    // The column names are repeated at the top of each page the table goes on to.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.matches("(Supplier) Tj").count() >= pages - 1);
}

#[test]
fn pdf_text_is_escaped_and_in_win_ansi() {
    assert_eq!(super::pdf::literal_for_test(r"a(b)\c"), r"a\(b\)\\c");
    assert_eq!(super::pdf::literal_for_test("£5 €6"), r"\2435 \2006");
    assert_eq!(super::pdf::literal_for_test("日本"), "??");
}

// ---- A knowledge page (DSK-KEX-1) ----

use super::page::{lay_out_page, plain, Block};
use ordin_service_api::commands::knowledge::KnowledgeExportFormat;

const PAGE_MD: &str = "\
# Suppliers

Rates are **agreed** each *quarter*; see [the rate card](https://example.org/rates) and [[Pricing]].

## How to check

1. Open the sheet
2. Compare with `last_rates`
   - a nested point

- one
- two

### Table

| Supplier | Rate |
|----------|------|
| Acme & Co | 412.50 |
| Brook | 380 |

```
let x = 1;
```
";

// spec: ordin-desktop/components/OrdinDesktop#DSK-KEX-1
#[test]
fn a_page_is_read_into_headings_paragraphs_lists_tables_and_code() {
    let page = lay_out_page("Supplier notes", PAGE_MD);
    assert_eq!(page.title, "Supplier notes");
    let headings: Vec<(u8, String)> = page
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading { level, runs } => Some((*level, plain(runs))),
            _ => None,
        })
        .collect();
    assert_eq!(
        headings,
        vec![
            (1, "Suppliers".to_owned()),
            (2, "How to check".to_owned()),
            (3, "Table".to_owned())
        ]
    );
    // Bold and italic kept; a link is its text then its address; a page link is its text only.
    let Block::Paragraph(runs) = &page.blocks[1] else {
        panic!("a paragraph: {:?}", page.blocks[1]);
    };
    assert_eq!(
        plain(runs),
        "Rates are agreed each quarter; see the rate card (https://example.org/rates) and Pricing."
    );
    assert!(runs.iter().any(|r| r.bold && r.text == "agreed"));
    assert!(runs.iter().any(|r| r.italic && r.text == "quarter"));
    let items: Vec<(bool, u64, u8, String)> = page
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Item {
                ordered,
                number,
                depth,
                runs,
                ..
            } => Some((*ordered, *number, *depth, plain(runs))),
            _ => None,
        })
        .collect();
    assert_eq!(
        items,
        vec![
            (true, 1, 0, "Open the sheet".to_owned()),
            (true, 2, 0, "Compare with last_rates".to_owned()),
            (false, 1, 1, "a nested point".to_owned()),
            (false, 1, 0, "one".to_owned()),
            (false, 2, 0, "two".to_owned()),
        ]
    );
    assert_eq!(page.lists.len(), 3);
    let table = page
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Table(rows) => Some(rows),
            _ => None,
        })
        .expect("a table");
    let cells: Vec<Vec<String>> = table
        .iter()
        .map(|row| row.iter().map(|c| plain(c)).collect())
        .collect();
    assert_eq!(
        cells,
        vec![
            vec!["Supplier", "Rate"],
            vec!["Acme & Co", "412.50"],
            vec!["Brook", "380"]
        ]
    );
    assert!(page.blocks.contains(&Block::Code("let x = 1;".to_owned())));
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-KEX-1
#[test]
fn a_page_as_word_has_real_headings_lists_and_tables() {
    let bytes = render_page("Supplier notes", PAGE_MD, KnowledgeExportFormat::Docx);
    let parts = unzip(&bytes);
    assert_well_formed(&parts);
    assert_targets_exist(&parts);
    assert!(part(&parts, "[Content_Types].xml").contains("wordprocessingml.numbering+xml"));
    assert!(part(&parts, "docProps/core.xml").contains("<dc:title>Supplier notes</dc:title>"));
    let document = part(&parts, "word/document.xml");
    assert!(document.contains(
        r#"<w:pStyle w:val="Title"/></w:pPr><w:r><w:t xml:space="preserve">Supplier notes</w:t>"#
    ));
    for style in ["Heading1", "Heading2", "Heading3"] {
        assert_eq!(
            document
                .matches(&format!(r#"<w:pStyle w:val="{style}"/>"#))
                .count(),
            1,
            "{style}"
        );
    }
    // Bold, italic and the link's address.
    assert!(document.contains(r#"<w:rPr><w:b/></w:rPr><w:t xml:space="preserve">agreed</w:t>"#));
    assert!(document.contains(r#"<w:rPr><w:i/></w:rPr><w:t xml:space="preserve">quarter</w:t>"#));
    assert!(document.contains("the rate card (https://example.org/rates)"));
    // Five list items, each numbered by its list; the nested one a level down.
    assert_eq!(document.matches("<w:numPr>").count(), 5);
    assert_eq!(
        document
            .matches(r#"<w:ilvl w:val="1"/><w:numId w:val="2"/>"#)
            .count(),
        1
    );
    let numbering = part(&parts, "word/numbering.xml");
    assert_eq!(numbering.matches("<w:num ").count(), 3);
    assert!(numbering.contains(r#"<w:num w:numId="1"><w:abstractNumId w:val="1"/>"#));
    assert!(numbering.contains(r#"<w:num w:numId="2"><w:abstractNumId w:val="0"/>"#));
    assert!(numbering.contains(r#"<w:numFmt w:val="decimal"/>"#));
    assert!(numbering.contains(r#"<w:numFmt w:val="bullet"/>"#));
    // A real table: a header row repeated on each page and two rows; the text escaped.
    assert_eq!(document.matches("<w:tbl>").count(), 1);
    let table = &document[document.find("<w:tbl>").unwrap()..document.find("</w:tbl>").unwrap()];
    assert_eq!(table.matches("<w:tr>").count(), 3);
    assert_eq!(table.matches("<w:tblHeader/>").count(), 1);
    assert!(table.contains("Acme &amp; Co"));
    // Code in a monospace font.
    assert!(document.contains(
        r#"<w:pStyle w:val="Code"/></w:pPr><w:r><w:t xml:space="preserve">let x = 1;</w:t>"#
    ));
    assert!(document.contains(r#"<w:rFonts w:ascii="Courier New""#));
    let styles = part(&parts, "word/styles.xml");
    for style in ["Heading2", "Heading3", "ListParagraph", "Code"] {
        assert!(
            styles.contains(&format!(r#"w:styleId="{style}""#)),
            "{style}"
        );
    }
}

// spec: ordin-desktop/components/OrdinDesktop#DSK-KEX-1
#[test]
fn a_page_as_pdf_is_a_paged_document() {
    let bytes = render_page("Supplier notes", PAGE_MD, KnowledgeExportFormat::Pdf);
    assert!(bytes.starts_with(b"%PDF-1.4"));
    assert!(bytes.ends_with(b"%%EOF\n"));
    let document = lopdf::Document::load_mem(&bytes).expect("a PDF");
    assert_eq!(document.get_pages().len(), 1);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("(Supplier notes) Tj"));
    // Bold in Helvetica-Bold, italic in Helvetica-Oblique, code in Courier.
    assert!(text.contains("/F2 10.5 Tf") && text.contains("(agreed) Tj"));
    assert!(text.contains("/F4 10.5 Tf") && text.contains("(quarter) Tj"));
    assert!(text.contains("/F3 9.0 Tf") && text.contains("(let x = 1;) Tj"));
    assert!(text.contains("/BaseFont /Courier "));
    assert!(text.contains("(\\225) Tj"), "a bullet");
    assert!(text.contains("(1.) Tj"));
    assert!(text.contains("(Acme & Co) Tj"));
}

#[test]
fn a_long_page_goes_on_over_several_pages() {
    let markdown: String = (0..300)
        .map(|i| format!("Paragraph {i} of a long page with enough words to fill a line.\n\n"))
        .collect();
    let bytes = render_page("Long", &markdown, KnowledgeExportFormat::Pdf);
    let document = lopdf::Document::load_mem(&bytes).expect("a PDF");
    let pages = document.get_pages().len();
    assert!(pages >= 5, "{pages} pages");
    // A dashboard's PDF still opens with the five fonts on every page.
    let dashboard = render("Prices", &report(3), &words(), DashboardExportFormat::Pdf);
    assert_eq!(
        lopdf::Document::load_mem(&dashboard)
            .expect("a PDF")
            .get_pages()
            .len(),
        1
    );
}

#[test]
fn a_page_file_is_named_after_its_title() {
    assert_eq!(
        page_file_name("Q3: suppliers", KnowledgeExportFormat::Pdf),
        "Q3  suppliers.pdf"
    );
    assert_eq!(
        page_file_name(" / ", KnowledgeExportFormat::Docx),
        "Page.docx"
    );
}
