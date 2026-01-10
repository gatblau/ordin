//! The field rules of the records this crate keeps (JournalAndArtefactStore.md *Shared Context*,
//! validation table, and Internal Logic 7.1, 8.1, 9.1, 10, 11.1, 12.1, 13.1). The Rust check is the
//! authority; the tables' `CHECK` constraints are a second line.

use ordin_types::common::InputUsed;
use ordin_types::domain::runs::{
    ColumnType, ReportData, ReportSection, ReportTableColumn, StartupReport,
};
use ordin_types::text::check_len;

use crate::error::JournalError;

/// `^[a-z][a-z0-9_]{0,63}$`: event and entry kinds.
pub(crate) fn is_kind(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// `[a-z_]{1,40}`: the component that wrote an entry.
pub(crate) fn is_component(raw: &str) -> bool {
    (1..=40).contains(&raw.len()) && raw.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// `^[A-Z][A-Z0-9_]{2,63}$`: a registry code carried in an entry's `data.code`.
pub(crate) fn is_code(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    (3..=64).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// Artefact.path: relative to its folder, 1–4096 bytes, no `..` segment, no leading `/`, no NUL.
pub(crate) fn artefact_path(raw: &str) -> Result<(), JournalError> {
    let refused = Err(JournalError::invalid("ART_PATH_INVALID"));
    if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') || raw.starts_with('/') {
        return refused;
    }
    if raw.split(['/', '\\']).any(|segment| segment == "..") {
        return refused;
    }
    Ok(())
}

/// Artefact.media_type:
/// `^[a-z0-9][a-z0-9!#$&^_.+-]{0,126}/[a-z0-9][a-z0-9!#$&^_.+-]{0,126}$`.
pub(crate) fn media_type(raw: &str) -> Result<(), JournalError> {
    fn part(p: &str) -> bool {
        let bytes = p.as_bytes();
        (1..=127).contains(&bytes.len())
            && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
            && bytes[1..]
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"!#$&^_.+-".contains(b))
    }
    match raw.split_once('/') {
        Some((kind, sub)) if part(kind) && part(sub) => Ok(()),
        _ => Err(JournalError::invalid("ART_MEDIA_TYPE_INVALID")),
    }
}

/// StartupReport lists (≤ 500 items, each ≤ 500 characters) and each reason (1–500 characters).
pub(crate) fn startup_report(report: &StartupReport) -> Result<(), JournalError> {
    let lists: [Vec<&str>; 5] = [
        report.enforced_rules.iter().map(String::as_str).collect(),
        report
            .rules_not_enforced
            .iter()
            .map(|r| r.rule.as_str())
            .collect(),
        report
            .unavailable_parts
            .iter()
            .map(String::as_str)
            .collect(),
        report.available_tools.iter().map(String::as_str).collect(),
        report
            .adapter_verbs_not_offered
            .iter()
            .map(String::as_str)
            .collect(),
    ];
    for list in &lists {
        if list.len() > 500 || list.iter().any(|item| item.chars().count() > 500) {
            return Err(JournalError::invalid("START_LIST_TOO_LONG"));
        }
    }
    for item in &report.rules_not_enforced {
        check_len(&item.reason, 1, 500, "START_REASON_MISSING")?;
    }
    Ok(())
}

/// Report serialised at most 2,000,000 bytes.
const MAX_REPORT_BYTES: usize = 2_000_000;
const MAX_ROWS: usize = 10_000;
const MAX_COLUMNS: usize = 30;
const MAX_SERIES: usize = 8;
const MAX_POINTS: usize = 5_000;
const MAX_PIE_SLICES: usize = 6;

/// Internal Logic 12.1: the structural rules in their stated order, then the size of the whole
/// serialised report (`serialised_len`).
pub(crate) fn report(
    title: &str,
    parts: &[ReportSection],
    serialised_len: usize,
) -> Result<(), JournalError> {
    check_len(title, 1, 200, "REPORT_TITLE_INVALID")?;
    if parts.is_empty() || parts.len() > 12 {
        return Err(JournalError::invalid("REPORT_PARTS_OUT_OF_RANGE"));
    }
    for part in parts {
        check_len(&part.heading, 1, 200, "REPORT_TITLE_INVALID")?;
        if part.sources.is_empty() {
            return Err(JournalError::invalid("REPORT_PART_NO_SOURCE"));
        }
    }
    for part in parts {
        match &part.data {
            ReportData::Table { columns, rows } => {
                if rows.len() > MAX_ROWS || columns.len() > MAX_COLUMNS {
                    return Err(JournalError::invalid("REPORT_TABLE_TOO_LARGE"));
                }
            }
            ReportData::Chart {
                columns,
                rows,
                series_columns,
                ..
            } => {
                if rows.len() > MAX_ROWS || columns.len() > MAX_COLUMNS {
                    return Err(JournalError::invalid("REPORT_TABLE_TOO_LARGE"));
                }
                // SPEC-GAP-3: "≤ 5,000 points" is read as rows times series (at least one series).
                let points = rows.len().saturating_mul(series_columns.len().max(1));
                if series_columns.len() > MAX_SERIES || points > MAX_POINTS {
                    return Err(JournalError::invalid("REPORT_CHART_TOO_LARGE"));
                }
            }
            ReportData::KeyFigures { .. } => {}
        }
    }
    for part in parts {
        if let ReportData::Chart {
            chart_kind, rows, ..
        } = &part.data
        {
            if *chart_kind == ordin_types::domain::runs::ChartKind::Pie
                && rows.len() > MAX_PIE_SLICES
            {
                return Err(JournalError::invalid("REPORT_PIE_TOO_MANY_SLICES"));
            }
        }
    }
    for part in parts {
        if let ReportData::KeyFigures { figures } = &part.data {
            if figures.is_empty() || figures.len() > 8 {
                return Err(JournalError::invalid("REPORT_FIGURES_OUT_OF_RANGE"));
            }
        }
    }
    for part in parts {
        let columns: &[ReportTableColumn] = match &part.data {
            ReportData::Table { columns, .. } | ReportData::Chart { columns, .. } => columns,
            ReportData::KeyFigures { .. } => &[],
        };
        for column in columns {
            let money = column.column_type == ColumnType::Money;
            let valid = match (&column.currency, money) {
                (Some(code), true) => {
                    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
                }
                (None, false) => true,
                _ => false,
            };
            if !valid {
                return Err(JournalError::invalid("REPORT_CURRENCY_MISMATCH"));
            }
        }
    }
    if serialised_len > MAX_REPORT_BYTES {
        return Err(JournalError::invalid("REPORT_TOO_LARGE"));
    }
    Ok(())
}

/// InputUsed limits (2C shared rule table): name 1–120, location 1–2048, version_read 1–256.
pub(crate) fn inputs(inputs: &[InputUsed]) -> Result<(), JournalError> {
    for input in inputs {
        check_len(&input.name, 1, 120, "INPUT_NAME_INVALID")?;
        check_len(&input.location, 1, 2048, "INPUT_LOCATION_INVALID")?;
        check_len(&input.version_read, 1, 256, "INPUT_VERSION_INVALID")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_components_and_codes_follow_their_forms() {
        assert!(is_kind("run_state_changed"));
        assert!(is_kind(&"a".repeat(64)));
        assert!(!is_kind(&"a".repeat(65)));
        assert!(!is_kind("Run"));
        assert!(!is_kind("1run"));
        assert!(!is_kind(""));
        assert!(is_component("rules_engine"));
        assert!(!is_component("rules-engine"));
        assert!(!is_component(&"a".repeat(41)));
        assert!(is_code("CRED_PROVIDER_REFUSED"));
        assert!(!is_code("cred"));
        assert!(!is_code("AB"));
    }

    #[test]
    fn artefact_paths_and_media_types() {
        assert!(artefact_path("out/summary.md").is_ok());
        for bad in ["", "/etc/passwd", "a/../b", "..", "a\0b", "a\\..\\b"] {
            assert!(artefact_path(bad).is_err(), "{bad:?}");
        }
        assert!(artefact_path(&"a".repeat(4097)).is_err());
        assert!(media_type("text/markdown").is_ok());
        assert!(media_type("application/vnd.ms-excel").is_ok());
        for bad in ["text", "Text/markdown", "/x", "x/", "a/b/c d"] {
            assert!(media_type(bad).is_err(), "{bad:?}");
        }
    }
}
