//! Internal Logic B.1: the card and proposal limits, checked in field order, with NFC
//! normalisation and the control-character refusal of 2C wire §0.

use ordin_types::text::normalise_text;
use ordin_types::wire::approval::{AffectedItem, ApprovalContent};

use crate::error::ApprovalError;
use crate::model::ApprovalRequest;

/// The fixed content labels of 11 §5.5, drawn from the message catalogue.
const FIXED_LABELS: [&str; 5] = [
    "See the changes",
    "See the page",
    "See the files",
    "See the command",
    "See what will be sent",
];

/// Words a verb may not be, compared ignoring case.
const FORBIDDEN_VERBS: [&str; 3] = ["ok", "yes", "continue"];

const QUESTION_MAX: usize = 120;
const VERB_MAX: usize = 32;
const WHERE_MAX: usize = 200;
const AFFECTS_MAX_ITEMS: usize = 50;
const AFFECTS_LABEL_MAX: usize = 200;
const CHANGES_MAX: usize = 1_000;
const MESSAGE_TEXT_MAX: usize = 100_000;
const COMMAND_MAX: usize = 8_000;
const FILES_MAX: usize = 100;
const ACTION_MAX: usize = 500;
const DESTINATION_MAX: usize = 2_048;
const CONSEQUENCE_MAX: usize = 2_000;
const RECORDS_MAX: usize = 1_000;

fn invalid(code: &'static str, field: &'static str) -> ApprovalError {
    ApprovalError::Invalid { code, field }
}

/// NFC-normalises `raw`; a control character other than `\n` and `\t` is refused with the
/// code of the field.
fn normalised(raw: &str, code: &'static str, field: &'static str) -> Result<String, ApprovalError> {
    normalise_text(raw).map_err(|_| invalid(code, field))
}

fn within(text: &str, min: usize, max: usize) -> bool {
    let count = text.chars().count();
    (min..=max).contains(&count)
}

/// One `AffectedItem`, normalised, with its label (a recipient's name) within 1–200 characters.
fn affected(
    item: &AffectedItem,
    code: &'static str,
    field: &'static str,
) -> Result<AffectedItem, ApprovalError> {
    let checked = |label: &str| -> Result<String, ApprovalError> {
        let label = normalised(label, code, field)?;
        if within(&label, 1, AFFECTS_LABEL_MAX) {
            Ok(label)
        } else {
            Err(invalid(code, field))
        }
    };
    Ok(match item {
        AffectedItem::File {
            label,
            version,
            place_id,
        } => AffectedItem::File {
            label: checked(label)?,
            version: version
                .as_deref()
                .map(|v| normalised(v, code, field))
                .transpose()?,
            place_id: *place_id,
        },
        AffectedItem::Record { label } => AffectedItem::Record {
            label: checked(label)?,
        },
        AffectedItem::Recipient {
            name,
            address,
            outside_company,
        } => AffectedItem::Recipient {
            name: checked(name)?,
            address: normalised(address, code, field)?,
            outside_company: *outside_company,
        },
    })
}

fn content(content: &ApprovalContent) -> Result<ApprovalContent, ApprovalError> {
    const LABEL: &str = "APPR_CONTENT_LABEL_INVALID";
    const LARGE: &str = "APPR_CONTENT_TOO_LARGE";
    let label_of = |label: &str| -> Result<String, ApprovalError> {
        let label = normalised(label, LABEL, "content")?;
        if FIXED_LABELS.contains(&label.as_str()) {
            Ok(label)
        } else {
            Err(invalid(LABEL, "content"))
        }
    };
    Ok(match content {
        ApprovalContent::Message {
            label,
            text,
            destination_note,
        } => {
            let label = label_of(label)?;
            let text = normalised(text, LARGE, "content")?;
            if !within(&text, 1, MESSAGE_TEXT_MAX) {
                return Err(invalid(LARGE, "content"));
            }
            ApprovalContent::Message {
                label,
                text,
                destination_note: destination_note
                    .as_deref()
                    .map(|note| normalised(note, LARGE, "content"))
                    .transpose()?,
            }
        }
        ApprovalContent::Command {
            label,
            command,
            runs_in,
        } => {
            let label = label_of(label)?;
            let command = normalised(command, LARGE, "content")?;
            if !within(&command, 1, COMMAND_MAX) {
                return Err(invalid(LARGE, "content"));
            }
            ApprovalContent::Command {
                label,
                command,
                runs_in: normalised(runs_in, LARGE, "content")?,
            }
        }
        ApprovalContent::Changes { label, change_set } => ApprovalContent::Changes {
            label: label_of(label)?,
            change_set: change_set.clone(),
        },
        ApprovalContent::Page { label, page } => ApprovalContent::Page {
            label: label_of(label)?,
            page: page.clone(),
        },
        ApprovalContent::Files { label, files } => {
            let label = label_of(label)?;
            if !(1..=FILES_MAX).contains(&files.len()) {
                return Err(invalid(LARGE, "content"));
            }
            ApprovalContent::Files {
                label,
                files: files.clone(),
            }
        }
        ApprovalContent::Email { label, email } => ApprovalContent::Email {
            label: label_of(label)?,
            email: email.clone(),
        },
    })
}

/// Returns the request with every text field NFC-normalised, or the first failure in field order.
pub(crate) fn validate(req: ApprovalRequest) -> Result<ApprovalRequest, ApprovalError> {
    let mut req = req;

    // question
    req.question = normalised(&req.question, "APPR_QUESTION_INVALID", "question")?;
    if !within(&req.question, 1, QUESTION_MAX) || !req.question.ends_with('?') {
        return Err(invalid("APPR_QUESTION_INVALID", "question"));
    }

    // verb
    req.verb = normalised(&req.verb, "APPR_VERB_INVALID", "verb")?;
    let lowered = req.verb.to_lowercase();
    if !within(&req.verb, 1, VERB_MAX) || FORBIDDEN_VERBS.contains(&lowered.as_str()) {
        return Err(invalid("APPR_VERB_INVALID", "verb"));
    }

    // where_line
    req.where_line = normalised(&req.where_line, "APPR_WHERE_INVALID", "where_line")?;
    if !within(&req.where_line, 1, WHERE_MAX) {
        return Err(invalid("APPR_WHERE_INVALID", "where_line"));
    }

    // affects
    if !(1..=AFFECTS_MAX_ITEMS).contains(&req.affects.len()) {
        return Err(invalid("APPR_AFFECTS_INVALID", "affects"));
    }
    req.affects = req
        .affects
        .iter()
        .map(|item| affected(item, "APPR_AFFECTS_INVALID", "affects"))
        .collect::<Result<Vec<_>, _>>()?;

    // changes
    req.changes = normalised(&req.changes, "APPR_CHANGES_INVALID", "changes")?;
    if !within(&req.changes, 1, CHANGES_MAX) {
        return Err(invalid("APPR_CHANGES_INVALID", "changes"));
    }

    // content
    if let Some(card_content) = &req.content {
        req.content = Some(content(card_content)?);
    }

    // show_details
    if req.show_details && req.content.is_none() {
        return Err(invalid("APPR_SHOW_DETAILS_WITHOUT_CONTENT", "show_details"));
    }

    // proposal
    req.proposal.action = normalised(&req.proposal.action, "APPR_ACTION_INVALID", "action")?;
    if !within(&req.proposal.action, 1, ACTION_MAX) {
        return Err(invalid("APPR_ACTION_INVALID", "action"));
    }
    req.proposal.destination = normalised(
        &req.proposal.destination,
        "APPR_DESTINATION_INVALID",
        "destination",
    )?;
    if !within(&req.proposal.destination, 1, DESTINATION_MAX) {
        return Err(invalid("APPR_DESTINATION_INVALID", "destination"));
    }
    req.proposal.consequence = normalised(
        &req.proposal.consequence,
        "APPR_CONSEQUENCE_INVALID",
        "consequence",
    )?;
    if !within(&req.proposal.consequence, 1, CONSEQUENCE_MAX) {
        return Err(invalid("APPR_CONSEQUENCE_INVALID", "consequence"));
    }
    // More than 1,000 affected records is the registry's `APPR_RECORDS_TOO_MANY`.
    if req.proposal.affects.len() > RECORDS_MAX {
        return Err(invalid("APPR_RECORDS_TOO_MANY", "affects"));
    }
    req.proposal.affects = req
        .proposal
        .affects
        .iter()
        .map(|item| affected(item, "APPR_AFFECTS_INVALID", "affects"))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(req)
}
