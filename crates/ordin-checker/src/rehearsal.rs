//! Step 33: each deterministic rehearsal expectation against the run's evidence (ADR-085). A
//! model-judged expectation is not an `ExpectationRule` and is never checked here.

use crate::model::{ExpectationOutcome, ExpectationRule, RunEvidence};

/// The longest evidence line; it never carries message content beyond this.
const MAX_EVIDENCE_CHARS: usize = 120;

fn clip(line: String) -> String {
    if line.chars().count() <= MAX_EVIDENCE_CHARS {
        line
    } else {
        line.chars().take(MAX_EVIDENCE_CHARS).collect()
    }
}

/// True when `text` holds a Markdown heading line (`#` to `######`) reading `heading`, ignoring case
/// and surrounding spaces.
fn has_heading(text: &str, heading: &str) -> bool {
    let wanted = heading.trim().to_lowercase();
    text.lines().any(|line| {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        (1..=6).contains(&hashes) && line[hashes..].trim().to_lowercase() == wanted
    })
}

fn times(n: usize) -> String {
    if n == 1 {
        "once".to_owned()
    } else {
        format!("{n} times")
    }
}

// spec: ordin-desktop/components/Checker#ADR-085
pub(crate) fn check(rules: &[ExpectationRule], evidence: &RunEvidence) -> Vec<ExpectationOutcome> {
    rules
        .iter()
        .map(|rule| {
            let (met, line) = match rule {
                ExpectationRule::ResultContains { text } => {
                    let met = evidence
                        .final_message
                        .to_lowercase()
                        .contains(&text.to_lowercase());
                    let line = if met {
                        "the result holds the text"
                    } else {
                        "the result does not hold the text"
                    };
                    (met, line.to_owned())
                }
                ExpectationRule::ResultHasSection { heading } => {
                    let met = has_heading(&evidence.final_message, heading);
                    let line = if met {
                        "the result has the section"
                    } else {
                        "the result has no such section"
                    };
                    (met, line.to_owned())
                }
                ExpectationRule::ToolUsed { tool } | ExpectationRule::ToolNotUsed { tool } => {
                    let used = evidence.tools_used.iter().filter(|t| *t == tool).count();
                    let met = match rule {
                        ExpectationRule::ToolUsed { .. } => used > 0,
                        _ => used == 0,
                    };
                    let line = if used == 0 {
                        format!("tool {tool} not used")
                    } else {
                        format!("tool {tool} used {}", times(used))
                    };
                    (met, line)
                }
                ExpectationRule::ApprovalAsked => (
                    evidence.approvals_asked > 0,
                    format!("{} approvals asked", evidence.approvals_asked),
                ),
                ExpectationRule::NoExternalAction => (
                    evidence.external_actions == 0,
                    format!("{} external actions", evidence.external_actions),
                ),
                ExpectationRule::ArtefactProduced { name_suffix } => {
                    let found = evidence
                        .artefact_names
                        .iter()
                        .find(|name| name.ends_with(name_suffix.as_str()));
                    match found {
                        Some(name) => (true, format!("artefact {name} produced")),
                        None => (false, format!("no artefact ending {name_suffix} produced")),
                    }
                }
            };
            ExpectationOutcome {
                rule: rule.clone(),
                met,
                evidence: clip(line),
            }
        })
        .collect()
}
