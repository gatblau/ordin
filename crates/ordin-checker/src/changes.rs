//! Section F: the form checks of a withdrawal and of a vocabulary change (Checker.md *Internal
//! Logic* 27–28). The checks that read agents are in the service.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use ordin_agent_model::{VocabularyAxis, VocabularySet, WithdrawnListEntry};
use regex::Regex;

use crate::context::Item;
use crate::finding::{finding, msg};
use crate::model::{Finding, Fix, Severity};
use crate::stage_one::cited_terms;

static CODE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^[a-z0-9-]{1,64}$").ok());

/// The status terms ordin's code gives meaning to (ADR-091 item 4).
const FIXED_STATUS_TERMS: [&str; 3] = ["draft", "approved", "deprecated"];

/// The most citing parts named in `args.parts` before "and N more".
const MAX_NAMED_PARTS: usize = 10;

fn withdrawal_failure(code: &str, entry: &WithdrawnListEntry, detail: String) -> Finding {
    finding(
        code,
        Severity::Failure,
        Some(&entry.part_identifier),
        msg::WITHDRAWAL_INVALID,
        &[("part", &entry.part_identifier)],
        vec![Fix::ChangeItYourself {
            field: "withdrawal".to_owned(),
        }],
        detail,
    )
}

/// The fields of an entry its file holds: the identity and times the source adds are left out.
fn file_fields(e: &WithdrawnListEntry) -> serde_json::Value {
    serde_json::json!([
        e.part_kind,
        e.part_identifier,
        e.part_version,
        e.part_content_digest.as_ref().map(|d| d.0.as_str()),
        e.reason,
        e.withdrawn_at.to_wire(),
        e.replacement_identifier,
        e.replacement_version,
    ])
}

/// Step 27, the list form: the proposed list is the existing one with exactly `entry` appended.
// Entries are compared on the fields the withdrawn-list file holds (kind, identifier,
// version, digest, reason, time withdrawn, replacement); the row identifier, repository and read times
// the source adds are not part of the file and are not compared.
// spec: ordin-desktop/components/Checker#UI-D99
pub(crate) fn list_form(
    entry: &WithdrawnListEntry,
    existing: &[WithdrawnListEntry],
    proposed: &[WithdrawnListEntry],
) -> Vec<Finding> {
    let mut out = Vec::new();
    let appended = proposed.len() == existing.len() + 1
        && existing
            .iter()
            .zip(proposed)
            .all(|(a, b)| file_fields(a) == file_fields(b))
        && proposed
            .last()
            .is_some_and(|last| file_fields(last) == file_fields(entry));
    if !appended {
        out.push(withdrawal_failure(
            "withdrawn_list_not_append",
            entry,
            "the proposed withdrawn list does not only append the new entry".to_owned(),
        ));
    }
    let duplicate = existing.iter().any(|e| {
        e.part_kind == entry.part_kind
            && e.part_identifier == entry.part_identifier
            && (e.part_version.is_none() || e.part_version == entry.part_version)
    });
    if duplicate {
        out.push(withdrawal_failure(
            "withdrawn_entry_duplicate",
            entry,
            format!("{} is already withdrawn", entry.part_identifier),
        ));
    }
    let reason_chars = entry.reason.chars().count();
    if !(1..=1000).contains(&reason_chars) {
        out.push(withdrawal_failure(
            "withdrawn_reason_invalid",
            entry,
            format!("the reason has {reason_chars} characters; 1 to 1,000"),
        ));
    }
    out
}

/// Step 27, the target: the part exists, and its version and digest agree with the entry.
pub(crate) fn target_form(entry: &WithdrawnListEntry, union: &[Item]) -> Vec<Finding> {
    let candidates: Vec<&Item> = union
        .iter()
        .filter(|i| i.kind == entry.part_kind && i.id() == entry.part_identifier)
        .collect();
    let target = match &entry.part_version {
        Some(version) => candidates
            .iter()
            .find(|i| i.manifest.metadata.version == *version)
            .copied(),
        None => candidates.first().copied(),
    };
    let Some(target) = target else {
        return vec![withdrawal_failure(
            "withdrawn_target_missing",
            entry,
            format!("{} is not a part of the repository", entry.part_identifier),
        )];
    };
    let mismatch = match (&entry.part_version, &entry.part_content_digest) {
        (None, None) => false,
        (Some(_), Some(digest)) => target
            .sourced
            .as_ref()
            .is_some_and(|s| s.part.content_digest != *digest),
        _ => true,
    };
    if mismatch {
        return vec![withdrawal_failure(
            "withdrawn_digest_mismatch",
            entry,
            "the version and digest must both be given and match the part".to_owned(),
        )];
    }
    Vec::new()
}

pub(crate) fn replacement_invalid(entry: &WithdrawnListEntry, detail: String) -> Finding {
    withdrawal_failure("withdrawn_replacement_invalid", entry, detail)
}

/// `agent_stops_on_withdrawal`: a warning, never a failure (UI-D99).
pub(crate) fn agent_stops(agent_id: &str, agent_name: &str, detail: String) -> Finding {
    finding(
        "agent_stops_on_withdrawal",
        Severity::Warning,
        Some(agent_id),
        msg::AGENT_STOPS,
        &[("agent", agent_name)],
        Vec::new(),
        detail,
    )
}

/// Comma-joined names, at most ten, then "and N more".
pub(crate) fn name_list(names: &[String]) -> String {
    let shown: Vec<&str> = names
        .iter()
        .take(MAX_NAMED_PARTS)
        .map(String::as_str)
        .collect();
    let mut text = shown.join(", ");
    if names.len() > MAX_NAMED_PARTS {
        text.push_str(&format!(" and {} more", names.len() - MAX_NAMED_PARTS));
    }
    text
}

fn vocabulary_failure(
    code: &str,
    message_id: &str,
    args: &[(&str, &str)],
    detail: String,
) -> Finding {
    finding(
        code,
        Severity::Failure,
        None,
        message_id,
        args,
        vec![Fix::ChangeItYourself {
            field: "terms".to_owned(),
        }],
        detail,
    )
}

/// Step 28, the form of the change and the citations within the repository.
// spec: ordin-desktop/components/Checker#ADR-091
pub(crate) fn vocabulary_form(
    axis: VocabularyAxis,
    added: &[(String, String, String)],
    retired: &[String],
    vocabulary: &VocabularySet,
    union: &[Item],
) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (code, label, description) in added {
        let problem = if !CODE.as_ref().is_some_and(|re| re.is_match(code)) {
            Some("the code must be 1 to 64 of a-z, 0-9 and -")
        } else if vocabulary.has(axis, code) || !seen.insert(code.clone()) {
            Some("the code already exists on the axis")
        } else if !(1..=120).contains(&label.chars().count()) {
            Some("the label must be 1 to 120 characters")
        } else if description.chars().count() > 1000 {
            Some("the description must be at most 1,000 characters")
        } else {
            None
        };
        if let Some(problem) = problem {
            out.push(vocabulary_failure(
                "vocabulary_code_invalid",
                msg::PART_INVALID,
                &[("part", code)],
                format!("{code:?}: {problem}"),
            ));
        }
    }
    for code in retired {
        if axis == VocabularyAxis::ArtefactStatus && FIXED_STATUS_TERMS.contains(&code.as_str()) {
            out.push(vocabulary_failure(
                "vocabulary_status_term_fixed",
                msg::VOCABULARY_STATUS_FIXED,
                &[("term", code)],
                format!("the status term {code:?} cannot be retired"),
            ));
            continue;
        }
        let citing: Vec<String> = union
            .iter()
            .filter(|item| {
                cited_terms(item.kind, &item.manifest)
                    .iter()
                    .any(|(a, c)| *a == axis && c == code)
            })
            .map(|item| item.name().to_owned())
            .collect();
        if !citing.is_empty() {
            let parts = name_list(&citing);
            out.push(vocabulary_failure(
                "vocabulary_term_cited",
                msg::VOCABULARY_TERM_CITED,
                &[("term", code), ("parts", &parts)],
                format!("{} parts of the repository cite {code:?}", citing.len()),
            ));
        }
    }
    out
}

/// `vocabulary_term_cited_elsewhere`: a warning naming the parts of other repositories.
pub(crate) fn cited_elsewhere(code: &str, names: &[String]) -> Finding {
    let parts = name_list(names);
    finding(
        "vocabulary_term_cited_elsewhere",
        Severity::Warning,
        None,
        msg::VOCABULARY_TERM_CITED,
        &[("term", code), ("parts", &parts)],
        Vec::new(),
        format!("{} parts of other repositories cite {code:?}", names.len()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_list_stops_at_ten() {
        let names: Vec<String> = (1..=12).map(|n| format!("p{n}")).collect();
        assert_eq!(
            name_list(&names),
            "p1, p2, p3, p4, p5, p6, p7, p8, p9, p10 and 2 more"
        );
        assert_eq!(name_list(&names[..2]), "p1, p2");
    }
}
