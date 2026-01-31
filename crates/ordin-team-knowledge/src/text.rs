//! The words of the share cards, the commit and the review request (§E steps 6–8, TW-A step 6,
//! TW-B step 3, TW-G step 2). No card word is branch, commit, push, pull, merge, rebase or conflict:
//! `GIT_WORDS` is checked against every sentence built here by a unit test.
//!
// SPEC-GAP-1: §E step 8 says the card's sentences are Fluent messages, but the crate graph allows this
// crate no edge to `ordin-localisation`; the en-GB sentences are built here, as the Push Service's own
// card text is, and the no-Git-words test covers them.

use ordin_push::CardText;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::TeamShareId;

use crate::model::TeamCounts;
use crate::store::Binding;
use crate::sync::Plan;

/// §E step 8's banned words.
#[cfg(test)]
pub const GIT_WORDS: &[&str] = &[
    "branch", "commit", "push", "pull", "merge", "rebase", "conflict",
];

/// The lines of the share-as-a-team-workspace card (TW-A step 6).
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceLines {
    pub places: Vec<String>,
    pub stays: Vec<String>,
    /// "Supplier analyst (can read and change Contracts)".
    pub agents: Vec<String>,
    pub results: String,
}

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// "and N more" after at most three names.
fn some_names(names: &[String], at_most: usize) -> String {
    if names.len() <= at_most {
        return names.join(", ");
    }
    format!(
        "{} and {} more",
        names[..at_most].join(", "),
        names.len() - at_most
    )
}

/// The parts of a counts sentence: "12 new facts", "2 changed pages", "1 page removed".
pub(crate) fn count_parts(c: &TeamCounts) -> Vec<String> {
    let mut parts = Vec::new();
    if c.new_facts > 0 {
        parts.push(plural(c.new_facts, "new fact", "new facts"));
    }
    if c.changed_facts > 0 {
        parts.push(plural(c.changed_facts, "changed fact", "changed facts"));
    }
    if c.removed_facts > 0 {
        parts.push(plural(c.removed_facts, "fact removed", "facts removed"));
    }
    if c.new_pages > 0 {
        parts.push(plural(c.new_pages, "new page", "new pages"));
    }
    if c.changed_pages > 0 {
        parts.push(plural(c.changed_pages, "changed page", "changed pages"));
    }
    if c.removed_pages > 0 {
        parts.push(plural(c.removed_pages, "page removed", "pages removed"));
    }
    parts
}

/// §E step 8's `affects` labels: "2 changed pages: Supplier rates, Board pack".
pub(crate) fn count_labels(c: &TeamCounts, changed_titles: &[String]) -> Vec<String> {
    let mut labels = Vec::new();
    for part in count_parts(c) {
        if part.contains("changed page") && !changed_titles.is_empty() {
            labels.push(format!("{part}: {}", some_names(changed_titles, 3)));
        } else {
            labels.push(part);
        }
    }
    labels
}

/// A question cut to 120 characters that still ends with "?".
pub(crate) fn question(text: String) -> String {
    if text.chars().count() <= 120 {
        return text;
    }
    let cut: String = text.chars().take(118).collect();
    format!("{}…?", cut.trim_end())
}

const HISTORY: &str = "What you share stays in the repository's history, even after it is deleted.";

fn sentence_of(plan: &Plan) -> String {
    let mut parts = count_parts(&plan.counts);
    if plan.description_changed {
        parts.push(format!(
            "{} to the workspace",
            plural(
                plan.workspace_subjects.len().max(1) as u32,
                "change",
                "changes"
            )
        ));
    }
    if !plan.results.is_empty() {
        parts.push(plural(plan.results.len() as u32, "result", "results"));
    }
    if plan.activity > 0 {
        parts.push(plural(
            plan.activity as u32,
            "thing done in this workspace",
            "things done in this workspace",
        ));
    }
    let mut s = parts.join(", ");
    if s.is_empty() {
        s = "Changes to this workspace".to_owned();
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => s,
    }
}

fn team_affects(plan: &Plan, affects: &mut Vec<AffectedItem>) {
    if plan.description_changed {
        let label = if plan.workspace_subjects.is_empty() {
            "Workspace changes: 1".to_owned()
        } else {
            format!(
                "Workspace changes: {}",
                some_names(&plan.workspace_subjects, 3)
            )
        };
        affects.push(AffectedItem::Record { label });
    }
    if plan.activity > 0 {
        affects.push(AffectedItem::Record {
            label: plural(
                plan.activity as u32,
                "thing done in this workspace",
                "things done in this workspace",
            ),
        });
    }
}

/// §E step 8 (scope `all`, with TW-B step 3, TW-F step 3 and TW-G step 2's additions).
pub(crate) fn share_card(b: &Binding, plan: &Plan) -> CardText {
    let mut affects = crate::sync::knowledge_affects(plan);
    team_affects(plan, &mut affects);
    for name in plan.results.iter().take(10) {
        affects.push(AffectedItem::Record {
            label: name.clone(),
        });
    }
    if plan.results.len() > 10 {
        affects.push(AffectedItem::Record {
            label: format!("and {} more", plan.results.len() - 10),
        });
    }
    let mut changes = format!("{}. {HISTORY}", sentence_of(plan));
    if plan.held_for_check > 0 {
        changes.push_str(&format!(
            " {} facts wait for you to check them.",
            plan.held_for_check
        ));
    }
    CardText {
        question: question(format!("Share what you've learned with {}?", b.repository)),
        verb: "Share".to_owned(),
        where_line: b.header_line(),
        affects,
        changes,
        content: None,
    }
}

/// TW-G step 2: a share of results after a run.
pub(crate) fn results_card(b: &Binding, plan: &Plan, workspace: &str) -> CardText {
    let q = match plan.results.len() {
        0 => format!("Share what was done in {workspace} with {}?", b.repository),
        1 => format!("Share 1 result with {}?", b.repository),
        n => format!("Share {n} results with {}?", b.repository),
    };
    let mut affects: Vec<AffectedItem> = plan
        .results
        .iter()
        .take(10)
        .map(|n| AffectedItem::Record { label: n.clone() })
        .collect();
    if plan.results.len() > 10 {
        affects.push(AffectedItem::Record {
            label: format!("and {} more", plan.results.len() - 10),
        });
    }
    team_affects(plan, &mut affects);
    CardText {
        question: question(q),
        verb: "Share".to_owned(),
        where_line: b.header_line(),
        affects,
        changes: format!("Your team will see them in Team activity. {HISTORY}"),
        content: None,
    }
}

/// TW-A step 6: the first share of a team workspace.
pub(crate) fn workspace_card(
    b: &Binding,
    plan: &Plan,
    workspace: &str,
    lines: &WorkspaceLines,
) -> CardText {
    let mut affects = vec![AffectedItem::Record {
        label: format!("Places: {}", lines.places.join(", ")),
    }];
    if !lines.stays.is_empty() {
        affects.push(AffectedItem::Record {
            label: format!("Stays on this computer: {}", lines.stays.join(", ")),
        });
    }
    for agent in &lines.agents {
        affects.push(AffectedItem::Record {
            label: format!("Agents: {agent}"),
        });
    }
    affects.extend(crate::sync::knowledge_affects(plan));
    affects.push(AffectedItem::Record {
        label: format!("Results will be saved in {}", lines.results),
    });
    CardText {
        question: question(format!(
            "Share {workspace} as a team workspace with {}?",
            b.repository
        )),
        verb: "Share".to_owned(),
        where_line: b.header_line(),
        affects,
        changes: format!(
            "Everyone who can open this folder can open this workspace with the same places and \
             agents, see its results and see what each person did, but never your conversations. {HISTORY}"
        ),
        content: None,
    }
}

/// "Team knowledge from [name]: [counts sentence]" and "Shared from ordin after approval" (§E step 6).
pub(crate) fn commit_message(name: &str, plan: &Plan) -> String {
    format!(
        "Team knowledge from {name}: {}\n\nShared from ordin after approval",
        sentence_of(plan)
    )
}

// SPEC-GAP-2: `TeamShareProposal.title` (1–256 characters) and `body` (≤ 65,536) are not worded by
// the spec; the review request takes the commit's first line and its second paragraph.
pub(crate) fn request_text(name: &str, plan: &Plan) -> (String, String) {
    let first = format!("Team knowledge from {name}: {}", sentence_of(plan));
    let title: String = first.chars().take(256).collect();
    (title, "Shared from ordin after approval".to_owned())
}

/// SA-TEAM-03, and TW-B step 5 for a team workspace: `<name> (<login>)`, at most 200 characters,
/// shortened in the name.
pub(crate) fn author_name(name: &str, login: Option<&str>) -> String {
    let Some(login) = login else {
        return name.chars().take(200).collect();
    };
    let suffix = format!(" ({login})");
    let room = 200usize.saturating_sub(suffix.chars().count());
    let short: String = name.chars().take(room).collect();
    format!("{}{suffix}", short.trim_end())
}

/// `ordin/team-knowledge/<first 8 hex characters of the TeamShareId's random part>`: the random
/// part of a UUIDv7 follows its 48-bit time and its version digit.
pub(crate) fn review_branch(id: TeamShareId) -> String {
    let simple = id.0.simple().to_string();
    let random: String = simple[13..16]
        .chars()
        .chain(simple[17..22].chars())
        .collect();
    format!("ordin/team-knowledge/{random}")
}
