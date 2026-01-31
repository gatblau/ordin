//! A dashboard's newest report written as a file on this computer (OrdinDesktop.md DSK-DBX-1 to
//! DSK-DBX-4; amended January 2026; user decision; Approved by the user January 2026): PDF,
//! PowerPoint, Excel and Word, and new slides added to the end of an existing PowerPoint
//! presentation. Everything is written here, with no other program and no service.
//!
//! Every part of the report becomes a table: a table part as it is, a chart part as the table of
//! the rows it is drawn from (as the dashboard's screen draws it, ScreensModule.md SM-DB-4), and key
//! figures as one row per figure. The Office files are written as Office Open XML with the `zip`
//! crate; the PDF is written directly with the 14 standard fonts, so no font is embedded.
//!
//! A knowledge page is written by the same PDF and Word writers (DSK-KEX-1; amended January 2026;
//! user decision; Approved by the user January 2026): its Markdown read by [`page`].

mod docx;
mod ooxml;
mod page;
mod pdf;
mod pptx;
mod xlsx;

#[cfg(test)]
mod tests;

use ordin_service_api::commands::dashboards::DashboardExportFormat;
use ordin_service_api::commands::knowledge::KnowledgeExportFormat;
use ordin_types::domain::{CellValue, ColumnType, Report, ReportData};

pub use pptx::{add_slides, AddSlidesError};

/// The words the files hold besides the report's own, in the active locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Words {
    /// The line under the title, e.g. "As of 10 January 2026, 10:43".
    pub as_of: String,
    /// The column headings of a key figures part.
    pub figure: String,
    pub value: String,
    pub unit: String,
}

impl Words {
    /// The en-GB words, for a build or a test without Localisation.
    pub fn english(as_of: String) -> Words {
        Words {
            as_of,
            figure: "Figure".to_owned(),
            value: "Value".to_owned(),
            unit: "Unit".to_owned(),
        }
    }
}

/// How a number cell is shown and stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberKind {
    Plain,
    /// Shown with this many decimal places.
    Decimals(u8),
    /// A fraction shown as a percentage (0.08 → 8%).
    Percentage,
}

/// One cell of a table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cell {
    Empty,
    Text(String),
    /// `raw` is the decimal as the report holds it; `shown` is how a document shows it.
    Number {
        raw: String,
        shown: String,
        kind: NumberKind,
    },
}

impl Cell {
    /// The text a document, a slide or a page shows.
    pub(crate) fn shown(&self) -> &str {
        match self {
            Cell::Empty => "",
            Cell::Text(text) => text,
            Cell::Number { shown, .. } => shown,
        }
    }
}

/// One part of the report, as a heading and a table.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Part {
    pub heading: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

/// The report ready to be written: the dashboard's name as title, the as-of line, the parts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sheet {
    pub title: String,
    pub subtitle: String,
    pub parts: Vec<Part>,
}

/// `report` laid out under the dashboard's `name`.
pub(crate) fn lay_out(name: &str, report: &Report, words: &Words) -> Sheet {
    let parts = report
        .parts
        .iter()
        .map(|section| {
            let (columns, rows) = match &section.data {
                ReportData::Table { columns, rows } | ReportData::Chart { columns, rows, .. } => {
                    let kinds: Vec<(ColumnType, Option<&str>, Option<u8>)> = columns
                        .iter()
                        .map(|c| (c.column_type, c.currency.as_deref(), c.decimals))
                        .collect();
                    let names = columns.iter().map(|c| c.name.clone()).collect();
                    let rows = rows
                        .iter()
                        .map(|row| {
                            (0..columns.len())
                                .map(|i| {
                                    let (kind, currency, decimals) = kinds[i];
                                    row.get(i).map_or(Cell::Empty, |value| {
                                        cell(value, kind, currency, decimals)
                                    })
                                })
                                .collect()
                        })
                        .collect();
                    (names, rows)
                }
                ReportData::KeyFigures { figures } => {
                    let names = vec![
                        words.figure.clone(),
                        words.value.clone(),
                        words.unit.clone(),
                    ];
                    let rows = figures
                        .iter()
                        .map(|f| {
                            vec![
                                Cell::Text(f.label.clone()),
                                number(&f.value.0, NumberKind::Plain, None),
                                if f.unit.is_empty() {
                                    Cell::Empty
                                } else {
                                    Cell::Text(f.unit.clone())
                                },
                            ]
                        })
                        .collect();
                    (names, rows)
                }
            };
            Part {
                heading: section.heading.clone(),
                columns,
                rows,
            }
        })
        .collect();
    Sheet {
        title: name.to_owned(),
        subtitle: words.as_of.clone(),
        parts,
    }
}

fn cell(value: &CellValue, kind: ColumnType, currency: Option<&str>, decimals: Option<u8>) -> Cell {
    match value {
        CellValue::Null => Cell::Empty,
        CellValue::Text(text) => Cell::Text(text.clone()),
        CellValue::Number(number_text) => match kind {
            ColumnType::Percentage => number(&number_text.0, NumberKind::Percentage, None),
            ColumnType::Money => number(&number_text.0, NumberKind::Decimals(2), currency),
            ColumnType::Number => number(
                &number_text.0,
                decimals.map_or(NumberKind::Plain, NumberKind::Decimals),
                None,
            ),
            ColumnType::Text | ColumnType::Date | ColumnType::DocumentLink => {
                Cell::Text(number_text.0.clone())
            }
        },
    }
}

/// A number cell; a value that is not a plain decimal is kept as text.
fn number(raw: &str, kind: NumberKind, currency: Option<&str>) -> Cell {
    let Ok(value) = raw.trim().parse::<f64>() else {
        return Cell::Text(raw.to_owned());
    };
    if !value.is_finite() {
        return Cell::Text(raw.to_owned());
    }
    let shown = match kind {
        NumberKind::Plain => raw.trim().to_owned(),
        NumberKind::Decimals(places) => format!("{value:.*}", usize::from(places)),
        NumberKind::Percentage => {
            let percent = format!("{:.2}", value * 100.0);
            let percent = percent.trim_end_matches('0').trim_end_matches('.');
            format!("{percent}%")
        }
    };
    let shown = match currency {
        Some(code) if !code.is_empty() => format!("{shown} {code}"),
        _ => shown,
    };
    Cell::Number {
        raw: raw.trim().to_owned(),
        shown,
        kind,
    }
}

/// The file's bytes for `format`.
pub fn render(
    name: &str,
    report: &Report,
    words: &Words,
    format: DashboardExportFormat,
) -> Vec<u8> {
    let sheet = lay_out(name, report, words);
    match format {
        DashboardExportFormat::Pdf => pdf::write(&sheet),
        DashboardExportFormat::Pptx => pptx::write(&sheet),
        DashboardExportFormat::Xlsx => xlsx::write(&sheet),
        DashboardExportFormat::Docx => docx::write(&sheet),
    }
}

/// The file name the save dialog proposes: the dashboard's name with the format's extension, every
/// character a file name cannot hold replaced by a space.
pub fn file_name(name: &str, format: DashboardExportFormat) -> String {
    let extension = match format {
        DashboardExportFormat::Pdf => "pdf",
        DashboardExportFormat::Pptx => "pptx",
        DashboardExportFormat::Xlsx => "xlsx",
        DashboardExportFormat::Docx => "docx",
    };
    format!("{}.{extension}", safe_stem(name))
}

/// A knowledge page's file bytes for `format` (DSK-KEX-1): `title` as the document's title, then
/// `markdown` laid out.
// spec: ordin-desktop/components/OrdinDesktop#DSK-KEX-1
pub fn render_page(title: &str, markdown: &str, format: KnowledgeExportFormat) -> Vec<u8> {
    let page = page::lay_out_page(title, markdown);
    match format {
        KnowledgeExportFormat::Pdf => pdf::write_page(&page),
        KnowledgeExportFormat::Docx => docx::write_page(&page),
    }
}

/// The file name the save dialog proposes for a knowledge page: its title, made safe as
/// [`safe_stem`] makes a dashboard's name, "Page" when nothing is left.
pub fn page_file_name(title: &str, format: KnowledgeExportFormat) -> String {
    let extension = match format {
        KnowledgeExportFormat::Pdf => "pdf",
        KnowledgeExportFormat::Docx => "docx",
    };
    format!("{}.{extension}", stem_or(title, "Page"))
}

/// `name` with `/ \ : * ? " < > |` and control characters replaced by a space, trimmed, cut to 120
/// characters; "Dashboard" when nothing is left.
pub fn safe_stem(name: &str) -> String {
    stem_or(name, "Dashboard")
}

fn stem_or(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .take(120)
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}
