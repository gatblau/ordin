//! Condition evaluation (RulesEngine.md *Internal Logic* C; wrightrun section I).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::facts::RunFacts;
use crate::load::{bare, compile, ControlSet};
use crate::store;
use crate::types::RulesHost;
use crate::vocab::{CallTarget, Condition, CountOf, DecimalShare, MatchMode};
use crate::{BUNDLE_DIR_PREFIXES, BUNDLE_FILES};

/// Why a condition could not be evaluated.
#[derive(Debug)]
pub(crate) enum EvalError {
    /// A host fact (`RULE_FACT_UNAVAILABLE`).
    Fact,
    /// A control-state statement (`RULE_STATE_UNAVAILABLE`).
    State,
}

/// Evaluates conditions for one run; counter and digest facts are asked once and remembered.
pub(crate) struct Evaluator<'a> {
    pub host: &'a dyn RulesHost,
    pub pool: Option<&'a SqlitePool>,
    pub set: &'a ControlSet,
    pub facts: &'a RunFacts,
    pub now_ms: i64,
    pub cache: HashMap<String, bool>,
    pub counts: HashMap<String, i64>,
}

fn word_shape() -> &'static Regex {
    static WORDS: OnceLock<Regex> = OnceLock::new();
    WORDS.get_or_init(|| Regex::new(r"\w+").unwrap_or_else(|_| unreachable!("constant")))
}

/// Words, lower-cased (Unicode word characters; `regex`'s `\w` is Unicode-aware).
pub(crate) fn words(text: &str) -> Vec<String> {
    word_shape()
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

/// SHA-256 hex of the text trimmed, inner white space collapsed to one space and lower-cased.
pub(crate) fn message_digest(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lowered = collapsed.to_lowercase();
    let digest = Sha256::digest(lowered.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A `0.50`–`1.00` share in hundredths; None when it is not one.
pub(crate) fn hundredths(share: &DecimalShare) -> Option<u32> {
    let (whole, fraction) = share.0.split_once('.').unwrap_or((share.0.as_str(), "0"));
    if whole.is_empty() || fraction.is_empty() || fraction.len() > 2 {
        return None;
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let whole: u32 = whole.parse().ok()?;
    let mut fraction: u32 = fraction.parse().ok()?;
    if share.0.split_once('.').is_some_and(|(_, f)| f.len() == 1) {
        fraction *= 10;
    }
    let value = whole.checked_mul(100)?.checked_add(fraction)?;
    (50..=100).contains(&value).then_some(value)
}

/// Whether a keyword (possibly several words) is found: all its words are in the text's set.
fn found(keyword: &str, text: &HashSet<String>) -> bool {
    let parts = words(keyword);
    !parts.is_empty() && parts.iter().all(|w| text.contains(w))
}

/// A normalised work-folder path: `/`-rooted, `.` and `..` resolved.
fn normalise(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let mut out = format!("/{}", parts.join("/"));
    if path.ends_with('/') && out != "/" {
        out.push('/');
    }
    out
}

/// A bundle path is read from the agent folder on disk; any other through the host.
fn is_bundle(path: &str) -> bool {
    let path = normalise(path);
    BUNDLE_DIR_PREFIXES.iter().any(|p| path.starts_with(p)) || BUNDLE_FILES.contains(&path.as_str())
}

fn on_disk(folder: &Path, path: &str) -> std::path::PathBuf {
    folder.join(normalise(path).trim_start_matches('/'))
}

impl Evaluator<'_> {
    /// Every condition holds, evaluated in order and stopping at the first that does not.
    pub(crate) async fn all(
        &mut self,
        conditions: &[Condition],
        command: Option<&str>,
    ) -> Result<bool, EvalError> {
        for condition in conditions {
            if !self.holds(condition, command).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) async fn holds(
        &mut self,
        condition: &Condition,
        command: Option<&str>,
    ) -> Result<bool, EvalError> {
        let key = self.facts.key();
        match condition {
            Condition::PathExists { path } => self.exists(path).await,
            Condition::PathAbsent { path } => Ok(!self.exists(path).await?),
            Condition::PathContains { path, text } => {
                if is_bundle(path) {
                    let file = on_disk(&self.set.folder, path);
                    match tokio::fs::read_to_string(&file).await {
                        Ok(content) => Ok(content.contains(text.as_str())),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                        Err(_) => Err(EvalError::Fact),
                    }
                } else {
                    self.host
                        .path_contains(&key, path, text)
                        .await
                        .map_err(|_| EvalError::Fact)
                }
            }
            Condition::RunInProgress => match command {
                None => Ok(false),
                Some(command) => Ok(self
                    .host
                    .runs_in_progress(command)
                    .await
                    .map_err(|_| EvalError::Fact)?
                    > 1),
            },
            Condition::StartCount {
                of,
                more_than,
                period_seconds,
            } => {
                let (kind, count_key) = match of {
                    CountOf::Command => ("command", command.map(str::to_owned)),
                    CountOf::Scope => ("scope", Some(self.facts.scope_text())),
                    CountOf::Asker => {
                        let connection = self
                            .facts
                            .scope
                            .as_deref()
                            .and_then(|s| s.split_once(':').map(|(c, _)| c.to_owned()));
                        let asker = self.facts.asker.clone();
                        (
                            "asker",
                            connection.zip(asker).map(|(c, a)| format!("{c}/{a}")),
                        )
                    }
                };
                let Some(count_key) = count_key else {
                    return Ok(false);
                };
                let cached = format!("start:{kind}:{count_key}:{period_seconds}");
                let count = match self.counts.get(&cached) {
                    Some(n) => *n,
                    None => {
                        let pool = self.pool.ok_or(EvalError::State)?;
                        let n = store::count_start(
                            pool,
                            kind,
                            &count_key,
                            *period_seconds,
                            self.now_ms,
                        )
                        .await
                        .map_err(|_| EvalError::State)?;
                        self.counts.insert(cached, n);
                        n
                    }
                };
                Ok(count > i64::from(*more_than))
            }
            Condition::RepeatedMessage { period_seconds } => {
                let cached = format!("message:{period_seconds}");
                if let Some(seen) = self.cache.get(&cached) {
                    return Ok(*seen);
                }
                let pool = self.pool.ok_or(EvalError::State)?;
                let digest = message_digest(&self.facts.text_cut);
                let seen = store::seen_message(
                    pool,
                    &self.facts.scope_text(),
                    &self.facts.asker_key,
                    &digest,
                    self.now_ms,
                    i64::from(*period_seconds) * 1_000,
                )
                .await
                .map_err(|_| EvalError::State)?;
                self.cache.insert(cached, seen);
                Ok(seen)
            }
            Condition::Keywords {
                words: list,
                r#match,
            } => {
                let text: HashSet<String> = words(&self.facts.text_cut).into_iter().collect();
                Ok(match r#match {
                    MatchMode::Any => list.iter().any(|k| found(k, &text)),
                    MatchMode::All => !list.is_empty() && list.iter().all(|k| found(k, &text)),
                })
            }
            Condition::KeywordShare { words: list, share } => {
                // Holds when at least `share` of the listed keywords are found in the text.
                let Some(needed) = hundredths(share) else {
                    return Ok(false);
                };
                let text: HashSet<String> = words(&self.facts.text_cut).into_iter().collect();
                let hits = list.iter().filter(|k| found(k, &text)).count();
                let total = list.len();
                Ok(total > 0 && hits * 100 >= usize::try_from(needed).unwrap_or(100) * total)
            }
            Condition::Pattern { patterns, r#match } => {
                let mut results = Vec::with_capacity(patterns.len());
                for pattern in patterns {
                    let matched = match self.set.patterns.get(pattern) {
                        Some(regex) => regex.is_match(&self.facts.text_cut),
                        None => compile(pattern)
                            .map(|regex| regex.is_match(&self.facts.text_cut))
                            .ok_or(EvalError::Fact)?,
                    };
                    results.push(matched);
                }
                Ok(match r#match {
                    MatchMode::Any => results.iter().any(|m| *m),
                    MatchMode::All => !results.is_empty() && results.iter().all(|m| *m),
                })
            }
            Condition::Setting { name, equals } => {
                let value = self
                    .host
                    .setting_value(self.facts.scope.as_deref(), name)
                    .await
                    .map_err(|_| EvalError::Fact)?;
                Ok(match &value {
                    serde_json::Value::Array(selected) => selected.contains(equals),
                    other => other == equals,
                })
            }
            Condition::Called {
                target,
                name,
                count,
            } => Ok(self.facts.called(&call_key(*target, name)) >= *count),
            Condition::NotCalled {
                target,
                name,
                count,
            } => Ok(self.facts.called(&call_key(*target, name)) < *count),
            Condition::Elapsed { at_least_seconds } => {
                Ok(self.now_ms - self.facts.started_at >= i64::from(*at_least_seconds) * 1_000)
            }
            Condition::ChangedWorkFolder => Ok(self.facts.changed_work_folder),
        }
    }

    async fn exists(&self, path: &str) -> Result<bool, EvalError> {
        if is_bundle(path) {
            match tokio::fs::metadata(on_disk(&self.set.folder, path)).await {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                Err(_) => Err(EvalError::Fact),
            }
        } else {
            self.host
                .path_exists(&self.facts.key(), path)
                .await
                .map_err(|_| EvalError::Fact)
        }
    }
}

/// The `tools_called` key of a `called` / `not-called` condition.
fn call_key(target: CallTarget, name: &str) -> String {
    match target {
        CallTarget::Tool => bare(name).to_owned(),
        CallTarget::Subagent => format!("task:{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_are_hundredths() {
        assert_eq!(hundredths(&DecimalShare("0.50".into())), Some(50));
        assert_eq!(hundredths(&DecimalShare("0.5".into())), Some(50));
        assert_eq!(hundredths(&DecimalShare("1.00".into())), Some(100));
        assert_eq!(hundredths(&DecimalShare("1".into())), Some(100));
        assert_eq!(hundredths(&DecimalShare("0.49".into())), None);
        assert_eq!(hundredths(&DecimalShare("x.5".into())), None);
        assert_eq!(hundredths(&DecimalShare("0.505".into())), None);
        assert_eq!(hundredths(&DecimalShare(".5".into())), None);
    }

    #[test]
    fn digests_ignore_case_and_spacing() {
        assert_eq!(
            message_digest("  Where is   the Policy? "),
            message_digest("where is the policy?")
        );
        assert_eq!(message_digest("a").len(), 64);
    }

    #[test]
    fn bundle_paths() {
        assert!(is_bundle("/AGENTS.md"));
        assert!(is_bundle("/knowledge/a.md"));
        assert!(is_bundle("knowledge/../brand/x"));
        assert!(!is_bundle("/.agent-run/triage/STOP"));
        assert_eq!(normalise("a/./b/../c/"), "/a/c/");
    }

    #[test]
    fn keywords_need_every_word() {
        let text: HashSet<String> = words("Where is the Holiday policy?").into_iter().collect();
        assert!(found("holiday policy", &text));
        assert!(!found("sick policy", &text));
        assert!(!found("", &text));
    }
}
