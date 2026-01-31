//! The content check (RulesEngine.md, amendment January 2026, ADR-107: *Internal Logic* L).

use std::time::Duration;

use ordin_types::domain::agent_profile::{CheckPoint, OnBlock, OnUnreachable};
use ordin_types::{AgentInstanceId, RunId};

use crate::engine::RulesEngine;
use crate::error::codes;
use crate::types::{CheckerVerdict, ContentCheckOutcome, ContentCheckRequest};

/// A checker's category is cut to this many characters (L.4).
const MAX_CATEGORY_CHARS: u32 = 64;

fn point_text(point: CheckPoint) -> &'static str {
    match point {
        CheckPoint::Input => "input",
        CheckPoint::Output => "output",
    }
}

/// The text cut to `max` characters, on a character boundary.
fn cut(text: &str, max: u32) -> String {
    text.chars()
        .take(usize::try_from(max).unwrap_or(usize::MAX))
        .collect()
}

impl RulesEngine {
    /// ADR-107. Runs every content-check control of the instance that covers `point`, in
    /// configuration order, and stops at the first block. Never fails: an engine error blocks.
    // spec: ordin-desktop/components/RulesEngine#ADR-107
    pub async fn check_content(
        &self,
        agent_instance_id: AgentInstanceId,
        run_id: RunId,
        point: CheckPoint,
        text: &str,
    ) -> ContentCheckOutcome {
        // L.1
        let Some(set) = self.set_of(agent_instance_id) else {
            tracing::warn!(
                agent_instance_id = agent_instance_id.to_string(),
                code = codes::RULESET_NOT_LOADED,
                "rules_content_check_refused"
            );
            return ContentCheckOutcome::Blocked {
                control: "app.ruleset".to_owned(),
                category: None,
                on_block: OnBlock::End,
                unreachable: false,
            };
        };
        // L.2
        let checks: Vec<_> = set
            .content_checks
            .iter()
            .filter(|c| c.points.contains(&point))
            .cloned()
            .collect();
        if checks.is_empty() {
            return ContentCheckOutcome::Passed;
        }
        // L.3
        let checker = self.host.content_checker();
        for check in checks {
            // L.4
            let request = ContentCheckRequest {
                point,
                run_id: run_id.to_string(),
                text: cut(text, check.max_chars),
            };
            let answer = match &checker {
                Some(checker) => {
                    checker
                        .check(
                            &check.connection,
                            &check.operation,
                            &request,
                            Duration::from_secs(u64::from(check.timeout_seconds)),
                        )
                        .await
                }
                None => Err(crate::types::CheckerUnreachable { reason: "refused" }),
            };
            match answer {
                Ok(reply) if reply.verdict == CheckerVerdict::Allow => {
                    self.record_check(run_id, &check.name, point, "allow", None)
                        .await;
                }
                Ok(reply) => {
                    let category = reply.category.map(|c| cut(&c, MAX_CATEGORY_CHARS));
                    let counts = check.categories.is_empty()
                        || category
                            .as_ref()
                            .is_some_and(|c| check.categories.contains(c));
                    if !counts {
                        // A category outside a non-empty list counts as allow.
                        self.record_check(run_id, &check.name, point, "allow", category.as_deref())
                            .await;
                        continue;
                    }
                    self.record_check(run_id, &check.name, point, "block", category.as_deref())
                        .await;
                    return ContentCheckOutcome::Blocked {
                        control: check.name.clone(),
                        category,
                        on_block: check.on_block,
                        unreachable: false,
                    };
                }
                // L.5
                Err(unreachable) => {
                    tracing::warn!(
                        control = check.name.as_str(),
                        point = point_text(point),
                        reason = unreachable.reason,
                        code = codes::CHECKER_UNREACHABLE,
                        "rules_checker_unreachable"
                    );
                    self.record_check(run_id, &check.name, point, "unreachable", None)
                        .await;
                    if check.on_unreachable == OnUnreachable::Block {
                        return ContentCheckOutcome::Blocked {
                            control: check.name.clone(),
                            category: None,
                            on_block: OnBlock::End,
                            unreachable: true,
                        };
                    }
                }
            }
        }
        ContentCheckOutcome::Passed
    }

    /// L.7: the journal entry (never the text) and metric family 50, once per check called.
    async fn record_check(
        &self,
        run_id: RunId,
        control: &str,
        point: CheckPoint,
        verdict: &str,
        category: Option<&str>,
    ) {
        self.journal_entry(
            "rules_content_check",
            Some(run_id),
            None,
            "The rules engine made a content check.",
            serde_json::json!({
                "run_id": run_id.to_string(),
                "control": control,
                "point": point_text(point),
                "verdict": verdict,
                "category": category,
            }),
        )
        .await;
        if let Some(sink) = self.metrics() {
            sink.add(
                "wrightrun_content_checks_total",
                &[("point", point_text(point)), ("verdict", verdict)],
                1,
            );
        }
    }
}
