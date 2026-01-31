//! Building findings, and the Fluent message ids the Checker owns (Checker.md *Internal Logic* 40).

use std::collections::BTreeMap;

use crate::model::{Finding, Fix, Severity};

/// The longest `detail` the `check_findings` table holds.
const MAX_DETAIL_CHARS: usize = 2000;

/// Step 40: the message ids; the text lives in the screens' Fluent files.
pub(crate) mod msg {
    pub const PART_INVALID: &str = "check-part-invalid";
    pub const PARTS_DISAGREE: &str = "check-parts-disagree";
    pub const CHOOSE_PACKS: &str = "check-choose-packs";
    pub const FORMAT_CANNOT_HOLD: &str = "check-format-cannot-hold";
    pub const NO_PLACE_IN_AGENT: &str = "check-no-place-in-agent";
    pub const WHOLE_REPOSITORY: &str = "check-whole-repository";
    pub const AGENT_WOULD_NOT_PASS: &str = "check-agent-would-not-pass";
    pub const TRUST_NEEDS_IT_APPROVAL: &str = "check-trust-needs-it-approval";
    pub const TOOL_NOT_ALLOWED: &str = "check-tool-not-allowed";
    pub const JOB_NOT_COVERED: &str = "check-job-not-covered";
    pub const NAME_COLLISION: &str = "check-name-collision";
    pub const TASK_NAME_NOT_VERB: &str = "check-task-name-not-verb";
    pub const TEXT_TOO_LONG: &str = "check-text-too-long";
    pub const DATE_AMBIGUOUS: &str = "check-date-ambiguous";
    pub const BORROWED_UNREACHABLE: &str = "check-borrowed-unreachable";
    pub const BORROWED_APM_UNSUPPORTED: &str = "check-borrowed-apm-unsupported";
    pub const BARE_COMMAND_NAME: &str = "check-bare-command-name";
    pub const PUBLISH_GATE_NOT_MET: &str = "check-publish-gate-not-met";
    pub const INSTALLED_PART_GONE: &str = "check-installed-part-gone";
    pub const INSTALLED_PART_WITHDRAWN: &str = "check-installed-part-withdrawn";
    pub const WITHDRAWAL_INVALID: &str = "check-withdrawal-invalid";
    pub const AGENT_STOPS: &str = "check-agent-stops";
    pub const VOCABULARY_TERM_CITED: &str = "check-vocabulary-term-cited";
    pub const VOCABULARY_STATUS_FIXED: &str = "check-vocabulary-status-fixed";
    pub const LIFECYCLE_INVALID: &str = "check-lifecycle-invalid";
}

/// Cuts a detail to the column's limit, on a character boundary.
fn clip(detail: String) -> String {
    if detail.chars().count() <= MAX_DETAIL_CHARS {
        detail
    } else {
        detail.chars().take(MAX_DETAIL_CHARS).collect()
    }
}

/// One finding. `args` are plain values for the sentence; `detail` is for Diagnostics only.
pub(crate) fn finding(
    code: &str,
    severity: Severity,
    part_identifier: Option<&str>,
    message_id: &str,
    args: &[(&str, &str)],
    fixes: Vec<Fix>,
    detail: String,
) -> Finding {
    Finding {
        code: code.to_owned(),
        severity,
        part_identifier: part_identifier
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
        message_id: message_id.to_owned(),
        args: args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        fixes,
        detail: clip(detail),
    }
}

/// A failure about one part, rendered "the part [name] …" (`check-part-invalid`).
// Step 40: the findings with no message of their own use `check-part-invalid` with `args.part`.
pub(crate) fn part_failure(
    code: &str,
    part_id: &str,
    part_name: &str,
    fixes: Vec<Fix>,
    detail: String,
) -> Finding {
    finding(
        code,
        Severity::Failure,
        Some(part_id),
        msg::PART_INVALID,
        &[("part", part_name)],
        fixes,
        detail,
    )
}

/// True when no finding has severity Failure.
pub(crate) fn passes(findings: &[Finding]) -> bool {
    findings.iter().all(|f| f.severity != Severity::Failure)
}

/// `(failures, warnings)`.
pub(crate) fn counts(findings: &[Finding]) -> (u32, u32) {
    let failures = findings
        .iter()
        .filter(|f| f.severity == Severity::Failure)
        .count();
    let warnings = findings.len() - failures;
    (
        u32::try_from(failures).unwrap_or(u32::MAX),
        u32::try_from(warnings).unwrap_or(u32::MAX),
    )
}

/// Step 2: more than `MAX_FINDINGS_PER_RUN` keeps the first 500 and adds `findings_truncated`.
pub(crate) fn truncate(mut findings: Vec<Finding>) -> Vec<Finding> {
    let max = crate::model::MAX_FINDINGS_PER_RUN;
    if findings.len() > max {
        let dropped = findings.len() - max;
        findings.truncate(max);
        findings.push(finding(
            "findings_truncated",
            Severity::Failure,
            None,
            msg::PART_INVALID,
            &[("part", "")],
            Vec::new(),
            format!("{dropped} further findings were not kept"),
        ));
    }
    findings
}

/// The display name of a part for a sentence: `metadata.name`, else its identifier.
pub(crate) fn display_name<'a>(name: &'a str, id: &'a str) -> &'a str {
    if name.trim().is_empty() {
        id
    } else {
        name
    }
}
