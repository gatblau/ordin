//! The words of approval cards and rules lines, from the bundled catalogue messages in the active
//! locale, never from a model (OrdinDesktop.md, amendment January 2026, DSK-REC-1 to DSK-REC-4).
//! A field of a proposal or a step is the field's own text, shown as data and not translated.

use ordin_crew_orchestrator::CrewView;
use ordin_localisation::{id, ArgValue, Localisation, MessageArgs, MessageId};
use ordin_rules::{CardWording, ControlLine, ControlSource, Proposal};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;

/// A proposal field passed as a message argument: 60 characters at most (DSK-REC-1).
const ARGUMENT_MAX: usize = 60;
/// `where_line` and a rules line: 200 characters at most (DSK-REC-1, DSK-REC-3).
const LINE_MAX: usize = 200;
/// `question`: the Approval Service's limit of 120 characters (DSK-REC-30).
const QUESTION_MAX: usize = 120;
/// The shortest argument cut DSK-REC-30 step 3 accepts in the active locale.
const QUESTION_ARGUMENT_MIN: usize = 2;
/// The locale whose words DSK-REC-30 step 3 falls back to.
const SOURCE_LOCALE: &str = "en-GB";
/// An `AffectedItem` label: the Approval Service's limit of 200 characters.
const LABEL_MAX: usize = 200;
/// `changes`: the Approval Service's limit of 1,000 characters.
const CHANGES_MAX: usize = 1_000;
/// The crew's purpose in `approval-crew-where` (DSK-REC-2).
const PURPOSE_MAX: usize = 190;
/// Gates listed as affected records: the Approval Service's limit of 50 items.
const AFFECTS_MAX: usize = 50;
/// The fixed content label of a shell command's card (ApprovalService.md, 11 §5.5).
const SEE_THE_COMMAND: &str = "See the command";

/// `text` trimmed and cut to `max` characters, with "…" as the last one when it was longer.
pub fn cut(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The catalogue words of one card or line, in the active locale.
pub struct Words<'a> {
    pub localisation: &'a Localisation,
}

impl Words<'_> {
    fn message(&self, id: MessageId, args: MessageArgs) -> String {
        self.localisation.message(id, &args)
    }

    /// A field as shown: trimmed; empty → `approval-rules-not-stated` (DSK-REC-1).
    fn field(&self, text: &str, max: usize) -> String {
        let text = cut(text, max);
        if text.is_empty() {
            self.message(id::APPROVAL_RULES_NOT_STATED, MessageArgs::new())
        } else {
            text
        }
    }

    /// A field passed as a message argument, cut to 60 characters.
    fn argument(&self, text: &str) -> ArgValue {
        ArgValue::Text(self.field(text, ARGUMENT_MAX))
    }

    /// The `question` of a card: the message with each argument cut to 60 characters; when that is
    /// longer than 120 characters, each argument is cut to the length the message leaves for it,
    /// so the question fits and still ends in "?". A locale that leaves fewer than 2 characters per
    /// argument gets the en-GB words.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-30
    fn question(&self, id: MessageId, fields: &[(&'static str, &str)]) -> String {
        let shown: Vec<(&'static str, String)> = fields
            .iter()
            .map(|(name, text)| (*name, self.field(text, ARGUMENT_MAX)))
            .collect();
        let with = |values: &[(&'static str, String)]| {
            values
                .iter()
                .fold(MessageArgs::new(), |args, (name, text)| {
                    args.with(name, ArgValue::Text(text.clone()))
                })
        };
        let first = self.message(id, with(&shown));
        if first.chars().count() <= QUESTION_MAX || shown.is_empty() {
            return first;
        }
        let empty: Vec<(&'static str, String)> = shown
            .iter()
            .map(|(name, _)| (*name, String::new()))
            .collect();
        let fitted = |format: &dyn Fn(MessageArgs) -> String| -> Option<String> {
            let fixed = format(with(&empty)).chars().count();
            let limit = (QUESTION_MAX.saturating_sub(fixed) / shown.len()).min(ARGUMENT_MAX);
            if limit < QUESTION_ARGUMENT_MIN {
                return None;
            }
            let cut_values: Vec<(&'static str, String)> = shown
                .iter()
                .map(|(name, text)| (*name, cut(text, limit)))
                .collect();
            Some(format(with(&cut_values)))
        };
        if let Some(text) = fitted(&|args| self.message(id, args)) {
            return text;
        }
        tracing::warn!(
            component = "desktop",
            message_id = id.as_str(),
            locale = self.localisation.active().locale.as_str(),
            "card_words_too_long"
        );
        fitted(&|args| self.localisation.message_in(SOURCE_LOCALE, id, &args))
            .unwrap_or_else(|| cut(&first, QUESTION_MAX))
    }

    fn record(&self, label: &str) -> AffectedItem {
        AffectedItem::Record {
            label: self.field(label, LABEL_MAX),
        }
    }

    /// `RulesHost::word` (DSK-REC-1).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-1
    pub fn proposal(&self, proposal: &Proposal) -> CardWording {
        let plain = MessageArgs::new;
        let (question, verb, where_line, affects, changes, content_label) = match proposal {
            Proposal::ShellCommand { runs_in, .. } => {
                let args = || plain().with("runs_in", self.argument(runs_in));
                (
                    self.question(id::APPROVAL_RULES_SHELL_QUESTION, &[("runs_in", runs_in)]),
                    id::APPROVAL_RULES_VERB_RUN,
                    self.field(runs_in, LINE_MAX),
                    vec![self.record(runs_in)],
                    self.message(id::APPROVAL_RULES_SHELL_CHANGES, args()),
                    Some(SEE_THE_COMMAND.to_owned()),
                )
            }
            // `kind` is not shown.
            Proposal::ExternalTool {
                tool, destination, ..
            } => (
                self.question(
                    id::APPROVAL_RULES_EXTERNAL_QUESTION,
                    &[("tool", tool), ("destination", destination)],
                ),
                id::APPROVAL_RULES_VERB_ALLOW,
                self.field(destination, LINE_MAX),
                vec![self.record(destination)],
                self.message(
                    id::APPROVAL_RULES_EXTERNAL_CHANGES,
                    plain().with("destination", self.argument(destination)),
                ),
                None,
            ),
            Proposal::PackageTool { server, tool } => (
                self.question(
                    id::APPROVAL_RULES_PACKAGE_QUESTION,
                    &[("tool", tool), ("server", server)],
                ),
                id::APPROVAL_RULES_VERB_ALLOW,
                self.field(server, LINE_MAX),
                vec![self.record(tool)],
                self.message(
                    id::APPROVAL_RULES_PACKAGE_CHANGES,
                    plain().with("server", self.argument(server)),
                ),
                None,
            ),
            Proposal::ApprovalListTool { tool } => {
                let args = || plain().with("tool", self.argument(tool));
                (
                    self.question(id::APPROVAL_RULES_LISTED_QUESTION, &[("tool", tool)]),
                    id::APPROVAL_RULES_VERB_ALLOW,
                    self.message(id::APPROVAL_RULES_WHERE_THIS_COMPUTER, plain()),
                    vec![self.record(tool)],
                    self.message(id::APPROVAL_RULES_LISTED_CHANGES, args()),
                    None,
                )
            }
            Proposal::ArgumentRulePause { rule, tool } => (
                self.question(id::APPROVAL_RULES_ARGUMENT_QUESTION, &[("tool", tool)]),
                id::APPROVAL_RULES_VERB_ALLOW,
                self.message(
                    id::APPROVAL_RULES_WHERE_RULE,
                    plain().with("rule", self.argument(rule)),
                ),
                vec![self.record(tool)],
                self.message(
                    id::APPROVAL_RULES_ARGUMENT_CHANGES,
                    plain().with("rule", self.argument(rule)),
                ),
                None,
            ),
            Proposal::AgentPolicyApproval { verb, tool } => (
                self.question(
                    id::APPROVAL_RULES_POLICY_QUESTION,
                    &[("verb", verb), ("tool", tool)],
                ),
                id::APPROVAL_RULES_VERB_ALLOW,
                self.message(id::APPROVAL_RULES_WHERE_THIS_COMPUTER, plain()),
                vec![self.record(tool)],
                self.message(
                    id::APPROVAL_RULES_POLICY_CHANGES,
                    plain().with("verb", self.argument(verb)),
                ),
                None,
            ),
            Proposal::EndGatesSetAside { gates } => {
                let listed = cut(&self.localisation.format_list(gates), CHANGES_MAX);
                let affects = if gates.is_empty() {
                    vec![self.record("")]
                } else {
                    gates
                        .iter()
                        .take(AFFECTS_MAX)
                        .map(|g| self.record(g))
                        .collect()
                };
                (
                    self.message(id::APPROVAL_RULES_GATES_QUESTION, plain()),
                    id::APPROVAL_RULES_VERB_FINISH,
                    self.message(id::APPROVAL_RULES_WHERE_THIS_RUN, plain()),
                    affects,
                    self.message(
                        id::APPROVAL_RULES_GATES_CHANGES,
                        plain().with("gates", ArgValue::Text(listed)),
                    ),
                    None,
                )
            }
        };
        card(
            question,
            self.message(verb, plain()),
            cut(&where_line, LINE_MAX),
            affects,
            cut(&changes, CHANGES_MAX),
            content_label,
        )
    }

    /// `CrewHost::word_step_approval` (DSK-REC-2). `member` is not used; `line` is the workflow's
    /// own text, shown as data.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-2
    pub fn crew_step(&self, view: &CrewView, step_label: &str, line: &str) -> CardWording {
        let changes = cut(line, CHANGES_MAX);
        let changes = if changes.is_empty() {
            self.message(id::APPROVAL_CREW_STEP_CHANGES, MessageArgs::new())
        } else {
            changes
        };
        card(
            self.question(id::APPROVAL_CREW_STEP_QUESTION, &[("step", step_label)]),
            self.message(id::APPROVAL_CREW_VERB_CARRY_ON, MessageArgs::new()),
            cut(
                &self.message(
                    id::APPROVAL_CREW_WHERE,
                    MessageArgs::new().with(
                        "purpose",
                        ArgValue::Text(self.field(&view.purpose, PURPOSE_MAX)),
                    ),
                ),
                LINE_MAX,
            ),
            vec![self.record(step_label)],
            changes,
            None,
        )
    }

    /// `RulesHost::plain_line` (DSK-REC-3).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-3
    pub fn rules_line(&self, line: &ControlLine) -> String {
        let id = match line.source {
            ControlSource::Package => id::RULES_LINE_PACKAGE,
            ControlSource::Configuration => id::RULES_LINE_CONFIGURATION,
            ControlSource::Application => id::RULES_LINE_APPLICATION,
            ControlSource::AgentPolicy => id::RULES_LINE_AGENT_POLICY,
        };
        let rule = if line.detail.trim().is_empty() {
            line.name.clone()
        } else {
            line.detail.clone()
        };
        let text = self.message(
            id,
            MessageArgs::new().with("rule", ArgValue::Text(rule.trim().to_owned())),
        );
        cut(&text, LINE_MAX)
    }
}

/// A `CardWording` whose `proposal` restates the card (DSK-REC-1): the question without its final
/// "?", the where line, the affected items and the changes.
fn card(
    question: String,
    verb: String,
    where_line: String,
    affects: Vec<AffectedItem>,
    changes: String,
    content_label: Option<String>,
) -> CardWording {
    let action = question
        .strip_suffix('?')
        .unwrap_or(question.as_str())
        .to_owned();
    CardWording {
        proposal: ActionProposal {
            action,
            destination: where_line.clone(),
            affects: affects.clone(),
            consequence: changes.clone(),
        },
        question,
        verb,
        where_line,
        affects,
        changes,
        content_label,
    }
}

/// `RefResolver::approval_verbs` (DSK-REC-14): the four approve-button words in every bundled
/// locale, duplicates removed.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-14
pub fn approval_verbs(localisation: &Localisation) -> Vec<String> {
    let mut verbs: Vec<String> = Vec::new();
    for locale in ordin_localisation::bundled_locales() {
        for id in [
            id::APPROVAL_RULES_VERB_RUN,
            id::APPROVAL_RULES_VERB_ALLOW,
            id::APPROVAL_RULES_VERB_FINISH,
            id::APPROVAL_CREW_VERB_CARRY_ON,
        ] {
            let verb = localisation.message_in(locale, id, &MessageArgs::new());
            if !verbs.contains(&verb) {
                verbs.push(verb);
            }
        }
    }
    verbs
}
