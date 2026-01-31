//! The input rules of Internal Logic steps 1, 6, 12, 14 and 21a. Nothing is written when one fails.

use ordin_types::domain::agent_profile::{RunRequest, RunStarter};

use crate::error::RunQueueError;
use crate::types::{
    NewRun, PendingKind, RunPause, MAX_ARGUMENTS_BYTES, MAX_MESSAGE_CHARS,
    MAX_SESSION_RECORD_BYTES, MAX_STARTER_ID_CHARS,
};

/// The most commands `paused_with_command` takes (step 21a).
pub(crate) const MAX_COMMANDS: usize = 100;
/// `ScheduleEntry.arguments` limit (step 6).
const MAX_FIRING_ARGUMENTS_CHARS: usize = 4_096;
/// A command slug is at most this long (step 1).
const MAX_COMMAND_CHARS: usize = 64;

fn bad(field: &'static str) -> RunQueueError {
    RunQueueError::BadInput { field }
}

fn chars_between(value: &str, min: usize, max: usize) -> bool {
    let count = value.chars().count();
    count >= min && count <= max
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters.
pub(crate) fn is_slug(value: &str) -> bool {
    value.len() <= MAX_COMMAND_CHARS
        && !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Step 1.
pub(crate) fn new_run(new: &NewRun) -> Result<(), RunQueueError> {
    let ids_ok = match &new.started_by {
        RunStarter::Caller { caller_id } => chars_between(caller_id, 1, MAX_STARTER_ID_CHARS),
        RunStarter::ScheduleEntry { entry } => chars_between(entry, 1, MAX_STARTER_ID_CHARS),
        RunStarter::Intake { intake } => chars_between(intake, 1, MAX_STARTER_ID_CHARS),
        RunStarter::ChatConnection { connection, asker } => {
            chars_between(connection, 1, MAX_STARTER_ID_CHARS)
                && chars_between(asker, 1, MAX_STARTER_ID_CHARS)
        }
    };
    if !ids_ok {
        return Err(bad("started_by"));
    }
    match &new.request {
        RunRequest::Message { text } => {
            if !chars_between(text, 1, MAX_MESSAGE_CHARS)
                || text.trim().is_empty()
                || text.contains('\0')
            {
                return Err(bad("text"));
            }
        }
        RunRequest::Command { command, arguments } => {
            if !is_slug(command) {
                return Err(bad("command"));
            }
            let total: usize = arguments.iter().map(|a| a.value.len()).sum();
            if total > MAX_ARGUMENTS_BYTES || arguments.iter().any(|a| a.value.contains('\0')) {
                return Err(bad("arguments"));
            }
        }
    }
    if let Some(run_id) = &new.run_id {
        if ordin_types::id::parse_v7(run_id).is_err() {
            return Err(bad("run_id"));
        }
    }
    if !new.gathered_context.is_object() {
        return Err(bad("gathered_context"));
    }
    Ok(())
}

/// Step 6.
pub(crate) fn firing(entry_key: &str, command: &str, arguments: &str) -> Result<(), RunQueueError> {
    if !chars_between(entry_key, 1, MAX_STARTER_ID_CHARS) {
        return Err(bad("entry_key"));
    }
    if !is_slug(command) {
        return Err(bad("command"));
    }
    if arguments.chars().count() > MAX_FIRING_ARGUMENTS_CHARS {
        return Err(bad("arguments"));
    }
    Ok(())
}

/// Step 14.
pub(crate) fn pause(pause: &RunPause) -> Result<(), RunQueueError> {
    if pause.outcome.pending.is_empty() {
        return Err(bad("pending"));
    }
    if pause
        .outcome
        .pending
        .iter()
        .any(|item| item.kind == PendingKind::RefusedEnding && item.approval_id.is_some())
    {
        return Err(bad("pending.approval_id"));
    }
    if pause.session_record.len() > MAX_SESSION_RECORD_BYTES {
        return Err(RunQueueError::SessionRecordTooLarge);
    }
    Ok(())
}

/// A run or thread identifier in the store's form, or `None` for a string that is not a UUID.
pub(crate) fn uuid_text(raw: &str) -> Option<String> {
    uuid::Uuid::try_parse(raw)
        .ok()
        .map(|id| id.hyphenated().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        for ok in ["run-go-no-go", "a", "a1-b2", "x9"] {
            assert!(is_slug(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "A", "a_b", "a b"] {
            assert!(!is_slug(bad), "{bad}");
        }
        assert!(!is_slug(&"a".repeat(65)));
        assert!(is_slug(&"a".repeat(64)));
    }

    #[test]
    fn uuid_text_normalises_case_and_refuses_junk() {
        assert_eq!(
            uuid_text("0192A000-0000-7000-8000-000000000000").as_deref(),
            Some("0192a000-0000-7000-8000-000000000000")
        );
        assert_eq!(uuid_text("not-a-uuid"), None);
    }
}
