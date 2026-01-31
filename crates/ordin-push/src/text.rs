//! The words on cards and in copy headers (PushService.md Internal Logic A.6 and *Configuration*).
//!
//! The spec composes these through Fluent messages `push-card-question-one`, `push-card-question-many`,
//! `push-card-verb`, `push-copy-header` and `push-copy-audience-unknown`; the en-GB text of each is in
//! `ordin-localisation/catalogues/en-GB/push.ftl` and is formatted by the localiser in `PushDeps`.

use ordin_localisation::{id, ArgValue, Localisation, MessageArgs};
use ordin_types::time::Timestamp;

pub(crate) const COPY_VERB: &str = "Publish";
pub(crate) const UPDATE_VERB: &str = "Update";
pub(crate) const SHARE_ACTION: &str = "Send changed documents back to where they came from";
pub(crate) const COPY_ACTION: &str = "Publish a copy";
pub(crate) const PR_ACTION: &str = "Open a pull request";
pub(crate) const TEAM_ACTION: &str = "Share team knowledge";

/// Cut `text` to at most `max` characters, ending in an ellipsis when it was cut.
pub(crate) fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// `push-card-question-one` / `push-card-question-many`: at most 120 characters, ending in "?".
/// `origin` names the one place the changes go back to; `None` selects the several-origins message.
pub(crate) fn question(loc: &Localisation, count: usize, origin: Option<&str>) -> String {
    let count = ArgValue::Count(count as u64);
    match origin {
        Some(origin) => {
            let args = |origin: &str| {
                MessageArgs::new()
                    .with("count", count.clone())
                    .with("origin", ArgValue::Text(origin.to_owned()))
            };
            let overhead = loc
                .message(id::PUSH_CARD_QUESTION_ONE, &args(""))
                .chars()
                .count();
            let room = 120usize.saturating_sub(overhead);
            loc.message(id::PUSH_CARD_QUESTION_ONE, &args(&cut(origin, room)))
        }
        None => loc.message(
            id::PUSH_CARD_QUESTION_MANY,
            &MessageArgs::new().with("count", count),
        ),
    }
}

/// `push-card-verb`: the label of the card's action.
pub(crate) fn verb(loc: &Localisation) -> String {
    loc.message(id::PUSH_CARD_VERB, &MessageArgs::new())
}

/// `push-copy-header` (ADR-069): the fixed header line of a copy.
pub(crate) fn copy_header(
    loc: &Localisation,
    name: &str,
    from: &str,
    branch: Option<&str>,
    at: Timestamp,
) -> String {
    let args = MessageArgs::new()
        .with("name", ArgValue::Text(name.to_owned()))
        .with("from", ArgValue::Text(from.to_owned()))
        .with(
            "has_branch",
            ArgValue::Text(if branch.is_some() { "yes" } else { "no" }.to_owned()),
        )
        .with("branch", ArgValue::Text(branch.unwrap_or("").to_owned()))
        .with("at", ArgValue::DateTime(at));
    loc.message(id::PUSH_COPY_HEADER, &args)
}

/// `push-copy-audience-unknown` (C-42): the audience cannot be named.
pub(crate) fn audience_unknown(loc: &Localisation, place: &str) -> String {
    loc.message(
        id::PUSH_COPY_AUDIENCE_UNKNOWN,
        &MessageArgs::new().with("place", ArgValue::Text(place.to_owned())),
    )
}

/// The file name a copy gets when the person did not choose one and a restart lost the choice.
pub(crate) fn default_copy_name(original_name: &str, extension: &str) -> String {
    let stem = original_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(original_name);
    format!("{stem}.{extension}")
}
