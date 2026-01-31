//! The three events of *Observability*. Counts and codes only: never file content, and never a path.

use crate::error::FormatError;
use crate::types::{Detection, FormatId};

/// `format_plugins.read`, info.
pub(crate) fn read_ok(detection: &Detection, parts: usize, mapping_items: usize, duration_ms: u64) {
    tracing::info!(
        format = detection.format.as_str(),
        declared_version = detection.declared_version.as_str(),
        version_tested = detection.version_tested,
        parts = parts as u64,
        mapping_items = mapping_items as u64,
        duration_ms,
        "format_plugins.read"
    );
}

/// `format_plugins.read`, warn.
pub(crate) fn read_failed(detection: Option<&Detection>, error: &FormatError, duration_ms: u64) {
    match detection {
        Some(d) => tracing::warn!(
            format = d.format.as_str(),
            declared_version = d.declared_version.as_str(),
            version_tested = d.version_tested,
            duration_ms,
            error_code = error.code(),
            "format_plugins.read"
        ),
        None => tracing::warn!(
            duration_ms,
            error_code = error.code(),
            "format_plugins.read"
        ),
    }
}

/// `format_plugins.write`, info.
pub(crate) fn write_ok(
    format: FormatId,
    parts: usize,
    starting_files: usize,
    loss_items: usize,
    duration_ms: u64,
) {
    tracing::info!(
        format = format.as_str(),
        parts = parts as u64,
        starting_files = starting_files as u64,
        loss_items = loss_items as u64,
        duration_ms,
        "format_plugins.write"
    );
}

/// `format_plugins.write`, warn.
pub(crate) fn write_failed(format: FormatId, error: &FormatError, duration_ms: u64) {
    tracing::warn!(
        format = format.as_str(),
        duration_ms,
        error_code = error.code(),
        "format_plugins.write"
    );
}

/// `format_plugins.detect`, debug.
pub(crate) fn detected(formats: &[Detection]) {
    let names: Vec<&str> = formats.iter().map(|d| d.format.as_str()).collect();
    tracing::debug!(formats = names.join(",").as_str(), "format_plugins.detect");
}
