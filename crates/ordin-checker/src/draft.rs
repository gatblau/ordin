//! Section E: the checks only an agent draft has (Checker.md *Internal Logic* 14–25). Pure: the
//! draft, its staged parts and the facts the caller passes.

use std::collections::{BTreeMap, BTreeSet};

use ordin_agent_model::{markdown_body, FixedPackEntry, Manifest, PartKind, VocabularyAxis};
use ordin_agent_model::{StagedPart, VocabularySet};

use crate::finding::{finding, msg, part_failure};
use crate::model::TargetFormat;
use crate::model::{ExpectationOutcome, Finding, Fix, FixAction, ReportItem, Severity};
use crate::stage_one::cited_terms;

/// Step 15: each mapping report item has no place in an ordin agent.
pub(crate) fn mapping(items: &[ReportItem]) -> Vec<Finding> {
    items
        .iter()
        .map(|item| {
            finding(
                "no_place_in_agent",
                Severity::Failure,
                item.part_identifier.as_deref(),
                msg::NO_PLACE_IN_AGENT,
                &[("what", &item.what)],
                vec![
                    Fix::LeaveItOut {
                        what: item.what.clone(),
                    },
                    Fix::AskOrdin,
                ],
                format!(
                    "{} has no place in an ordin agent ({})",
                    item.what, item.reason
                ),
            )
        })
        .collect()
}

/// W7: a part of the draft cites a term its vocabulary marks retired.
pub(crate) fn retired_citations(
    parts: &[StagedPart],
    retired: &[(VocabularyAxis, String)],
) -> Vec<Finding> {
    let mut out = Vec::new();
    for staged in parts {
        let manifest = &staged.part.manifest;
        for (axis, code) in cited_terms(staged.part.kind, manifest) {
            if retired.contains(&(axis, code.clone())) {
                out.push(part_failure(
                    "vocabulary_term_retired",
                    &manifest.metadata.id,
                    crate::finding::display_name(&manifest.metadata.name, &manifest.metadata.id),
                    vec![Fix::ChangeItYourself {
                        field: "metadata".to_owned(),
                    }],
                    format!("the part cites the retired term {code:?}"),
                ));
            }
        }
    }
    out
}

/// One pack the space holds: (identifier, class).
pub(crate) type PackClass = (String, String);

/// Step 19: each required rule is settled by the maker's fixed packs, or left for the agent to ask.
// spec: ordin-desktop/components/Checker#ADR-089
pub(crate) fn fixed_packs(
    root: &Manifest,
    packs: &[PackClass],
    vocabulary: &VocabularySet,
) -> Vec<Finding> {
    let rules = root.required_practice_packs().unwrap_or_default();
    let fixed = root.fixed_practice_packs().unwrap_or_default();
    let mut out = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let mut candidates: Vec<String> = match &rule.class {
            Some(class) => packs
                .iter()
                .filter(|(_, c)| c == class)
                .map(|(id, _)| id.clone())
                .collect(),
            None => rule.ids.clone(),
        };
        candidates.sort();
        candidates.dedup();
        let fixed_count = fixed
            .iter()
            .filter(|entry| matches!(entry, FixedPackEntry::Pack(id) if candidates.contains(id)))
            .count();
        let asked = fixed.iter().any(|entry| match entry {
            FixedPackEntry::Ask { class, ids } => match (&rule.class, class) {
                (Some(rule_class), Some(class)) => rule_class == class,
                (None, None) => {
                    let wanted: BTreeSet<&String> = rule.ids.iter().collect();
                    !ids.is_empty() && ids.iter().all(|id| wanted.contains(id))
                }
                _ => false,
            },
            FixedPackEntry::Pack(_) => false,
        });
        let at_least = usize::try_from(rule.at_least).unwrap_or(usize::MAX);
        if fixed_count >= at_least || asked {
            continue;
        }
        let rule_index = u32::try_from(index).unwrap_or(u32::MAX);
        let class_words = match &rule.class {
            Some(class) => vocabulary
                .label(VocabularyAxis::PackClass, class)
                .unwrap_or_else(|| class.clone()),
            None => "working practice".to_owned(),
        };
        out.push(finding(
            "fixed_packs_unsatisfied",
            Severity::Failure,
            Some(&root.metadata.id),
            msg::CHOOSE_PACKS,
            &[("class", &class_words)],
            vec![
                Fix::ChoosePacks {
                    rule_index,
                    choices: candidates.clone(),
                },
                Fix::LetAgentAsk { rule_index },
            ],
            format!(
                "required rule {index} needs {} packs; {fixed_count} fixed and no ask entry",
                rule.at_least
            ),
        ));
    }
    out
}

/// Step 20: every job has at least one task, and each task it names is one of the draft's.
pub(crate) fn jobs_covered(
    jobs: &[String],
    tasks: &[String],
    original_tasks: &[String],
    job_tasks: &BTreeMap<u32, Vec<String>>,
) -> Vec<Finding> {
    let known: BTreeSet<String> = tasks
        .iter()
        .chain(original_tasks)
        .map(|t| t.trim().to_lowercase())
        .collect();
    let mut out = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        let named = u32::try_from(index)
            .ok()
            .and_then(|i| job_tasks.get(&i))
            .filter(|t| !t.is_empty());
        let covered = named.is_some_and(|names| {
            names
                .iter()
                .all(|n| known.contains(&n.trim().to_lowercase()))
        });
        if !covered {
            out.push(finding(
                "job_not_covered",
                Severity::Failure,
                None,
                msg::JOB_NOT_COVERED,
                &[("job", job)],
                vec![
                    Fix::AskOrdin,
                    Fix::ChangeItYourself {
                        field: "tasks".to_owned(),
                    },
                ],
                format!("job index {index} has no task"),
            ));
        }
    }
    out
}

/// Step 21: the draft's name equals another agent's of the repository, ignoring case and spaces.
pub(crate) fn name_collision(name: &str, others: &[String]) -> Option<Finding> {
    let wanted = name.trim().to_lowercase();
    others
        .iter()
        .any(|other| other.trim().to_lowercase() == wanted)
        .then(|| {
            finding(
                "name_collision",
                Severity::Failure,
                None,
                msg::NAME_COLLISION,
                &[("name", name.trim())],
                vec![Fix::ChangeItYourself {
                    field: "name".to_owned(),
                }],
                "another agent of the repository has this name".to_owned(),
            )
        })
}

/// Step 22: what the format cannot hold, and borrowing in an APM repository.
// spec: ordin-desktop/components/Checker#ADR-087
pub(crate) fn format(
    target: TargetFormat,
    loss: &[ReportItem],
    borrowed: &[String],
) -> Vec<Finding> {
    let mut out: Vec<Finding> = loss
        .iter()
        .map(|item| {
            finding(
                "format_loss",
                Severity::Failure,
                item.part_identifier.as_deref(),
                msg::FORMAT_CANNOT_HOLD,
                &[("format", "APM"), ("what", &item.what)],
                vec![
                    Fix::LeaveItOut {
                        what: item.what.clone(),
                    },
                    Fix::PublishToOrdinFormat,
                ],
                format!("APM format cannot hold {} ({})", item.what, item.reason),
            )
        })
        .collect();
    if target == TargetFormat::Apm && !borrowed.is_empty() {
        out.push(finding(
            "borrowed_apm_unsupported",
            Severity::Failure,
            None,
            msg::BORROWED_APM_UNSUPPORTED,
            &[("count", &borrowed.len().to_string())],
            vec![Fix::PublishToOrdinFormat],
            format!(
                "an APM repository cannot borrow parts; the draft borrows {}",
                borrowed.len()
            ),
        ));
    }
    out
}

/// The Markdown body of a staged part, None for a YAML part.
fn body_of(staged: &StagedPart) -> Option<String> {
    let path = &staged.part.manifest_path;
    let (key, bytes) = staged
        .part
        .files
        .0
        .iter()
        .filter(|(key, _)| path.ends_with(key.as_str()))
        .max_by_key(|(key, _)| key.len())?;
    markdown_body(key, bytes)
}

/// The command names `/<slug>` written bare in `body`, in first-seen order.
fn bare_names(body: &str, commands: &BTreeSet<String>, agent: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut found = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        if *c != '/' || (i > 0 && !chars[i - 1].is_whitespace()) {
            continue;
        }
        let slug: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || **c == '-')
            .collect();
        let after = chars.get(i + 1 + slug.chars().count());
        let ends = after.is_none_or(|c| c.is_whitespace() || c.is_ascii_punctuation());
        // `/<agent>:<slug>` is the qualified form, not a bare name.
        let qualified = slug == agent && after == Some(&':');
        if ends && !qualified && commands.contains(&slug) && !found.contains(&slug) {
            found.push(slug);
        }
    }
    found
}

/// Step 24 (R-94): a bare command name in a Markdown body offers the agent-qualified one.
pub(crate) fn bare_commands(parts: &[StagedPart], agent: &str) -> Vec<Finding> {
    let commands: BTreeSet<String> = parts
        .iter()
        .filter(|s| s.part.kind == PartKind::Command)
        .map(|s| s.part.manifest.slug())
        .collect();
    let mut out = Vec::new();
    if commands.is_empty() {
        return out;
    }
    for staged in parts {
        let Some(body) = body_of(staged) else {
            continue;
        };
        let id = &staged.part.manifest.metadata.id;
        for slug in bare_names(&body, &commands, agent) {
            let from = format!("/{slug}");
            let to = format!("/{agent}:{slug}");
            out.push(finding(
                "bare_command_name",
                Severity::Warning,
                Some(id),
                msg::BARE_COMMAND_NAME,
                &[("command", &slug)],
                vec![Fix::FixIt {
                    action: FixAction::PointToReplacement {
                        field: id.clone(),
                        from: from.clone(),
                        to: to.clone(),
                    },
                }],
                format!("{id} names the command {from} without its agent"),
            ));
        }
    }
    out
}

/// Step 25: an agent with a publish gate needs a rehearsal whose every expectation was met.
// spec: ordin-desktop/components/Checker#ADR-090
pub(crate) fn publish_gate(
    root: &Manifest,
    rehearsal: Option<&[ExpectationOutcome]>,
) -> Vec<Finding> {
    if root.publish_gate().is_none_or(|g| g.trim().is_empty()) {
        return Vec::new();
    }
    let not_met = |reason: &str, detail: String| {
        finding(
            "publish_gate_not_met",
            Severity::Failure,
            Some(&root.metadata.id),
            msg::PUBLISH_GATE_NOT_MET,
            &[("reason", reason)],
            vec![Fix::AskOrdin],
            detail,
        )
    };
    match rehearsal {
        None => vec![not_met(
            "not rehearsed",
            "the agent has a publish gate and no rehearsal".to_owned(),
        )],
        Some(outcomes) => outcomes
            .iter()
            .filter(|o| !o.met)
            .map(|o| {
                not_met(
                    "expectation not met",
                    format!(
                        "expectation not met: {}",
                        serde_json::to_string(&o.rule).unwrap_or_default()
                    ),
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_skip_qualified_and_partial_tokens() {
        let commands: BTreeSet<String> = ["apply".to_owned(), "cspa-431".to_owned()].into();
        let body = "run /apply next, then /cspa-431:apply and /applying or x/apply. /apply";
        assert_eq!(bare_names(body, &commands, "cspa-431"), vec!["apply"]);
    }

    #[test]
    fn name_collision_ignores_case_and_spaces() {
        assert!(name_collision(" Contract Checker ", &["contract checker".to_owned()]).is_some());
        assert!(name_collision("Other", &["contract checker".to_owned()]).is_none());
    }
}
