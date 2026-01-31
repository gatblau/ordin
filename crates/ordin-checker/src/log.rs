//! The log events of Checker.md *Observability* (SA-OBS-01). Each event carries codes, identifiers,
//! counts and timings only: never part text, file content or the person's words.
// spec: ordin-desktop/components/Checker#SA-OBS-01

use std::time::Duration;

use crate::error::CheckError;

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

pub(crate) struct RunLine<'a> {
    pub check_run_id: String,
    pub subject_kind: &'a str,
    pub passed: bool,
    pub failures: u32,
    pub warnings: u32,
    pub tidied: usize,
    pub parts: usize,
    pub duration: Duration,
}

pub(crate) fn check_run(line: &RunLine<'_>) {
    tracing::info!(
        check_run_id = line.check_run_id.as_str(),
        subject_kind = line.subject_kind,
        passed = line.passed,
        failures = line.failures,
        warnings = line.warnings,
        tidied = count(line.tidied),
        parts = count(line.parts),
        duration_ms = millis(line.duration),
        "checker.check_run"
    );
}

pub(crate) fn check_failed(subject_kind: &str, error: &CheckError) {
    tracing::warn!(
        subject_kind = subject_kind,
        error_code = error.code(),
        retryable = error.retryable(),
        "checker.check_failed"
    );
}

pub(crate) fn preexisting_finding(code: &str, part_identifier: Option<&str>) {
    tracing::debug!(
        code = code,
        part_identifier = part_identifier.unwrap_or(""),
        "checker.preexisting_finding"
    );
}

pub(crate) fn executable_parts_decided(agent_instance_id: &str, trusted: usize, needs: usize) {
    tracing::info!(
        agent_instance_id = agent_instance_id,
        trusted = count(trusted),
        needs_it_approval = count(needs),
        "checker.executable_parts_decided"
    );
}

pub(crate) fn trust_reevaluated(instances: usize, lost: usize, gained: usize) {
    tracing::info!(
        instances = count(instances),
        lost = count(lost),
        gained = count(gained),
        "checker.trust_reevaluated"
    );
}

/// An error, because it means the program's start-up order is wrong.
pub(crate) fn source_not_bound(operation: &str) {
    tracing::error!(operation = operation, "checker.source_not_bound");
}

pub(crate) fn trust_policy_invalid(profile_version: Option<&str>) {
    tracing::error!(
        profile_version = profile_version.unwrap_or(""),
        "checker.trust_policy_invalid"
    );
}

pub(crate) fn handover_checked(passed: bool, failed_codes: &[&str]) {
    tracing::info!(
        passed = passed,
        failed_codes = failed_codes.join(",").as_str(),
        "checker.handover_checked"
    );
}

/// A journal record that could not be written: the run row is the record, so the report is returned.
pub(crate) fn journal_failed(kind: &str, error_code: &str) {
    tracing::error!(
        kind = kind,
        error_code = error_code,
        "checker.journal_failed"
    );
}
