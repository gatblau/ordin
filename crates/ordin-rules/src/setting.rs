//! `check_setting_rule` (RulesEngine.md *Internal Logic* J; wrightrun section D), and the reading
//! of setting rules at a run's start (B.5).

use std::sync::OnceLock;

use regex::Regex;

use crate::error::RulesError;
use crate::eval::words;
use crate::load::{compile, ControlSet};
use crate::vocab::{AdditiveOutcome, Condition, DecidingOutcome, MessageRule};

const MAX_REPLY_CHARS: usize = 4_000;
const MAX_KEYWORDS: usize = 50;
const MAX_KEYWORD_CHARS: usize = 50;
const MAX_PATTERNS: usize = 10;
const PRIORITY_BOUND: i16 = 1_000;

fn slug() -> &'static Regex {
    static SLUG: OnceLock<Regex> = OnceLock::new();
    SLUG.get_or_init(|| {
        Regex::new(r"^[a-z0-9][a-z0-9-]{0,63}$").unwrap_or_else(|_| unreachable!("constant"))
    })
}

fn outcome_kind(outcome: &DecidingOutcome) -> &'static str {
    match outcome {
        DecidingOutcome::End { .. } => "end",
        DecidingOutcome::Reply { .. } => "reply",
        DecidingOutcome::Command { .. } => "command",
        DecidingOutcome::Tools { .. } => "tools",
        DecidingOutcome::SingleCall { .. } => "single-call",
    }
}

fn additive_kind(outcome: &AdditiveOutcome) -> &'static str {
    match outcome {
        AdditiveOutcome::AttachAction { .. } => "attach-action",
        AdditiveOutcome::Delivery { .. } => "delivery",
    }
}

/// Why a rule kept as a setting is refused, or None. `allowed` None skips the outcome-kind list (B.5).
pub(crate) fn refusal(
    set: &ControlSet,
    allowed: Option<&[String]>,
    rule: &MessageRule,
) -> Option<&'static str> {
    if !slug().is_match(&rule.name) {
        return Some("name");
    }
    if set.control_names.contains(&rule.name) {
        return Some("name-taken");
    }
    if !(-PRIORITY_BOUND..=PRIORITY_BOUND).contains(&rule.priority) {
        return Some("priority");
    }
    if rule.conditions.is_empty() || (rule.decides.is_none() && rule.adds.is_empty()) {
        return Some("shape");
    }
    if let Some(command) = &rule.command {
        if !set.commands.contains(command) {
            return Some("unknown-command");
        }
    }
    for condition in &rule.conditions {
        match condition {
            Condition::Keywords { words: list, .. }
            | Condition::KeywordShare { words: list, .. } => {
                let fits = (1..=MAX_KEYWORDS).contains(&list.len())
                    && list.iter().all(|w| {
                        (1..=MAX_KEYWORD_CHARS).contains(&w.chars().count()) && !words(w).is_empty()
                    });
                if !fits {
                    return Some("keywords");
                }
                if let Condition::KeywordShare { share, .. } = condition {
                    let ok = crate::eval::hundredths(share).is_some();
                    if !ok {
                        return Some("share");
                    }
                }
            }
            Condition::Pattern { patterns, .. } => {
                if !(1..=MAX_PATTERNS).contains(&patterns.len())
                    || patterns.iter().any(|p| compile(p).is_none())
                {
                    return Some("pattern");
                }
            }
            Condition::Setting { equals, .. } => {
                if !(equals.is_string() || equals.is_number() || equals.is_boolean()) {
                    return Some("setting");
                }
            }
            _ => return Some("condition-kind"),
        }
    }
    if let Some(decides) = &rule.decides {
        let kind = outcome_kind(decides);
        if kind == "end" || kind == "tools" {
            return Some("outcome-kind");
        }
        if allowed.is_some_and(|list| !list.iter().any(|k| k == kind)) {
            return Some("outcome-kind");
        }
        match decides {
            DecidingOutcome::Reply { text }
                if !(1..=MAX_REPLY_CHARS).contains(&text.chars().count()) =>
            {
                return Some("reply");
            }
            DecidingOutcome::Command { command } | DecidingOutcome::SingleCall { command }
                if !set.commands.contains(command) =>
            {
                return Some("unknown-command");
            }
            _ => {}
        }
    }
    for add in &rule.adds {
        if allowed.is_some_and(|list| !list.iter().any(|k| k == additive_kind(add))) {
            return Some("outcome-kind");
        }
    }
    // `InstanceRuleSet` carries no list of actions an `attach-action` may name, so an action's name
    // is not checked here.
    None
}

/// J.
pub(crate) fn check(
    set: &ControlSet,
    setting: &str,
    allowed_outcomes: &[String],
    rule: &MessageRule,
) -> Result<(), RulesError> {
    match refusal(set, Some(allowed_outcomes), rule) {
        None => Ok(()),
        Some(_) => Err(RulesError::SettingRuleNotAllowed(format!(
            "{setting}/{}",
            rule.name.chars().take(64).collect::<String>()
        ))),
    }
}
