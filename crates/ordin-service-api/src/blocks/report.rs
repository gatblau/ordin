//! The stored form of a checked report (Internal Logic 13; Journal `Report`, 2C part 1).

use ordin_types::common::AgentRef;
use ordin_types::domain::runs as stored;
use ordin_types::num::DecimalString;
use ordin_types::{AgentInstanceId, ReportId, RunId, WorkspaceId};

use super::{
    BarOrientation, Cell, ChartBlock, ChartKind, ColumnType, KeyFiguresBlock, ReportBlock,
    ReportPartContent, ReportPartSource, TableBlock, TableColumn,
};

/// A number as the stored family writes it (2C: numbers as `DecimalString`).
fn decimal(value: f64) -> DecimalString {
    DecimalString(format!("{value}"))
}

/// The JSON of a reference, as the stored family keeps it in text.
fn reference_json<T: serde::Serialize>(item: &T) -> String {
    serde_json::to_string(item).unwrap_or_default()
}

fn column(c: &TableColumn) -> stored::ReportTableColumn {
    let (column_type, currency, decimals) = match &c.column_type {
        ColumnType::Text => (stored::ColumnType::Text, None, None),
        ColumnType::Number { decimals } => (stored::ColumnType::Number, None, *decimals),
        ColumnType::Money { currency } => (stored::ColumnType::Money, Some(currency.clone()), None),
        ColumnType::Percentage => (stored::ColumnType::Percentage, None, None),
        ColumnType::Date => (stored::ColumnType::Date, None, None),
        ColumnType::DocumentLink => (stored::ColumnType::DocumentLink, None, None),
    };
    stored::ReportTableColumn {
        name: c.name.clone(),
        column_type,
        currency,
        decimals,
    }
}

fn cell(c: &Cell) -> stored::CellValue {
    match c {
        Cell::Empty => stored::CellValue::Null,
        Cell::Number(v) => stored::CellValue::Number(decimal(*v)),
        Cell::Text(s) => stored::CellValue::Text(s.clone()),
        Cell::Link(r) => stored::CellValue::Text(reference_json(r)),
    }
}

fn rows(rows: &[Vec<Cell>]) -> Vec<Vec<stored::CellValue>> {
    rows.iter().map(|r| r.iter().map(cell).collect()).collect()
}

/// The sources of one part. `WorkedOut.agent` is a placeholder the caller replaces with the run's
/// `AgentRef` after the call (13: "filled by the caller after the call").
fn sources(source: &ReportPartSource) -> Vec<stored::ReportSource> {
    match source {
        ReportPartSource::Documents { documents } => documents
            .iter()
            .map(|d| stored::ReportSource::Document {
                location: reference_json(&d.item),
                version_read: d.version.clone(),
            })
            .collect(),
        ReportPartSource::CompanySystem { system, read_at } => {
            vec![stored::ReportSource::CompanySystem {
                system: system.clone(),
                read_at: *read_at,
            }]
        }
        ReportPartSource::EarlierResult { run_id } => {
            vec![stored::ReportSource::EarlierResult { run_id: *run_id }]
        }
        ReportPartSource::WorkedOut { from, .. } => vec![stored::ReportSource::WorkedOutBy {
            agent: AgentRef::AgentInstance {
                // The nil UUID (`Uuid::default()`), replaced by the caller.
                agent_instance_id: AgentInstanceId(Default::default()),
            },
            from: from
                .iter()
                .map(reference_json)
                .collect::<Vec<_>>()
                .join(", "),
        }],
    }
}

fn table(t: &TableBlock) -> (stored::ReportData, Vec<stored::ReportSource>) {
    (
        stored::ReportData::Table {
            columns: t.columns.iter().map(column).collect(),
            rows: rows(&t.rows),
        },
        sources(&t.source),
    )
}

fn chart(c: &ChartBlock) -> (stored::ReportData, Vec<stored::ReportSource>) {
    let (chart_kind, bar_arrangement) = match c.chart {
        ChartKind::Bar {
            orientation: BarOrientation::Vertical,
            arrangement,
        } => (stored::ChartKind::BarVertical, Some(arrangement)),
        ChartKind::Bar {
            orientation: BarOrientation::Horizontal,
            arrangement,
        } => (stored::ChartKind::BarHorizontal, Some(arrangement)),
        ChartKind::Line => (stored::ChartKind::Line, None),
        ChartKind::Area => (stored::ChartKind::Area, None),
        ChartKind::Scatter => (stored::ChartKind::Scatter, None),
        ChartKind::Pie => (stored::ChartKind::Pie, None),
    };
    (
        stored::ReportData::Chart {
            chart_kind,
            columns: c.columns.iter().map(column).collect(),
            rows: rows(&c.rows),
            x_column: c.x.clone(),
            series_columns: c.series.clone(),
            x_title: c.x_title.clone().unwrap_or_default(),
            y_title: c.y_title.clone().unwrap_or_default(),
            bar_arrangement,
        },
        sources(&c.source),
    )
}

fn figures(k: &KeyFiguresBlock) -> (stored::ReportData, Vec<stored::ReportSource>) {
    (
        stored::ReportData::KeyFigures {
            figures: k
                .figures
                .iter()
                .map(|f| stored::ReportKeyFigure {
                    label: f.label.clone(),
                    value: decimal(f.value),
                    unit: f.unit.clone().unwrap_or_default(),
                    change_against: f.change.as_ref().map(|c| stored::ReportFigureChange {
                        earlier_label: c.against.clone(),
                        earlier_value: decimal(c.earlier_value),
                    }),
                })
                .collect(),
        },
        sources(&k.source),
    )
}

/// The stored form of a checked report (Journal `Report`, 2C part 1). A `WorkedOut` source's agent
/// is a placeholder the caller fills with the run's `AgentRef` after the call.
pub fn report_to_domain(
    block: &ReportBlock,
    report_id: ReportId,
    workspace_id: WorkspaceId,
    run_id: RunId,
) -> stored::Report {
    stored::Report {
        id: report_id,
        workspace_id,
        run_id,
        title: block.title.clone(),
        as_of: block.as_of,
        parts: block
            .parts
            .iter()
            .map(|part| {
                let (data, sources) = match &part.content {
                    ReportPartContent::Table(t) => table(t),
                    ReportPartContent::Chart(c) => chart(c),
                    ReportPartContent::KeyFigures(k) => figures(k),
                };
                stored::ReportSection {
                    heading: part.heading.clone(),
                    data,
                    sources,
                }
            })
            .collect(),
    }
}
