//! Before each tool call, and the commands ordin runs itself (RulesEngine.md *Internal Logic* D,
//! D2, K.1, K.2 and K.3).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use ordin_approval::{ApprovalRequest, WaitOutcome};
use ordin_types::domain::places::Grantee;
use ordin_types::domain::runs::{ApprovalState, ToolRequestDecision};
use ordin_types::encoding::canonical_json;
use ordin_types::num::Sha256Hex;
use ordin_types::wire::approval::{ApprovalAsker, ApprovalContent, RejectStyle};
use ordin_types::wire::rules::{ActionProposal, RuleSource, ToolCall, ToolOrigin, Verdict};
use ordin_types::{ApprovalId, PlaceId, RunId, ToolRequestId};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::args;
use crate::engine::{
    now_ms, read, source_text, Decided, RulesEngine, FOLDER_REASON, NOT_LOADED_REASON,
};
use crate::error::{codes, RulesError};
use crate::eval::{EvalError, Evaluator};
use crate::facts::RunFacts;
use crate::load::{bare, ControlSet};
use crate::policy;
use crate::store;
use crate::types::{
    ApprovalNeed, ApprovalWait, CallContext, CommandOrigin, CommandRequest, EmailCheck,
    EmailVerdict, EmailViolation, ProgramKind, Proposal, RulesOutcome, ToolClass,
};
use crate::vocab::{ArgumentForm, ArgumentOutcome, Point};
use crate::MAX_ARGUMENT_BYTES;

const MAX_TOOL_CHARS: usize = 128;
const MAX_SERVER_CHARS: usize = 64;
const MAX_COMMAND_CHARS: usize = 8_000;
const COMMAND_TOOL: &str = "command";
const SEE_THE_COMMAND: &str = "See the command";

const NOT_OFFERED: &str = "the tool is not offered in this run";
const NO_COMMANDS: &str = "this agent may not run commands";
const REJECTED: &str = "the person rejected this action";
const APPROVAL_DOWN: &str = "waiting for the person's approval could not start";
const MALFORMED: &str = "the tool call is not well formed";
const RUN_UNKNOWN: &str = "the run is not known";
const MISMATCH: &str = "the call does not belong to this session";
const NOT_RECORDED: &str = "the decision could not be recorded";
const INTERNAL: &str = "a declared control could not be evaluated";
const EXTERNAL_OFF: &str = "This kind of action is switched off on this server.";
const SLACK_OFF: &str = "Slack posts are switched off on this server.";
const CREW_OFF: &str = "This crew member may not act outside ordin.";
const PERSON_UNDECLARED: &str = "Only the project's declared checks run from here.";
const REVIEWER: &str = "the reviewer may only read";

fn server_shape() -> &'static Regex {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    SHAPE.get_or_init(|| {
        Regex::new(r"^[a-zA-Z0-9_.-]+$").unwrap_or_else(|_| unreachable!("constant"))
    })
}

fn name_shape() -> &'static Regex {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    SHAPE.get_or_init(|| {
        Regex::new(r"^[A-Za-z][A-Za-z0-9_-]{0,63}$").unwrap_or_else(|_| unreachable!("constant"))
    })
}

/// SHA-256 hex of the canonical JSON (Logging R-3).
pub(crate) fn digest_of(value: &Value) -> (Sha256Hex, usize) {
    let bytes = canonical_json(value);
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    (Sha256Hex::from_digest(&digest), bytes.len())
}

fn fail_closed(rule: &str, why: &str, code: &'static str) -> Decided {
    Decided::deny(RuleSource::FailClosed, rule, why, Some(code))
}

fn decision_of(decided: &Decided) -> ToolRequestDecision {
    match decided.verdict {
        Verdict::Allow => ToolRequestDecision::Allow,
        Verdict::Deny { .. } => ToolRequestDecision::Deny,
        Verdict::Ask { .. } => ToolRequestDecision::Ask,
    }
}

/// The text of a string argument, or None.
fn text_arg<'a>(arguments: &'a Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str)
}

/// One ask the checks found (D.7), before the card is opened.
struct Asking {
    source: RuleSource,
    rule_id: String,
    proposal: Proposal,
    content: Option<(String, String)>,
    leaves_company: bool,
    /// A pause by an argument rule, for metric family 13.
    rule_source: Option<crate::types::ControlSource>,
}

/// What the evaluation of one call produced.
struct Evaluated {
    decided: Decided,
    facts: Option<RunFacts>,
    set: Option<Arc<ControlSet>>,
    /// (control, package/configuration) of a declared control that refused or paused.
    metric: Option<(&'static str, String, crate::types::ControlSource)>,
}

impl Evaluated {
    fn bare(decided: Decided) -> Evaluated {
        Evaluated {
            decided,
            facts: None,
            set: None,
            metric: None,
        }
    }
}

fn source_label(source: crate::types::ControlSource) -> &'static str {
    match source {
        crate::types::ControlSource::Configuration => "configuration",
        _ => "package",
    }
}

impl RulesEngine {
    /// DecisionPoint::BeforeToolCall. Never fails: any internal error denies (fail closed).
    pub async fn decide_tool_call(&self, call: &ToolCall, ctx: &CallContext) -> RulesOutcome {
        self.decide_coded(call, ctx).await.0
    }

    /// `decide_tool_call` with the registry code of a coded denial (the code is otherwise only logged).
    pub(crate) async fn decide_coded(
        &self,
        call: &ToolCall,
        ctx: &CallContext,
    ) -> (RulesOutcome, Option<&'static str>) {
        // D.1
        let (digest, size) = digest_of(&call.arguments);
        let request = match self
            .journal
            .record_tool_request(call.run_id, &call.tool, digest.clone())
            .await
        {
            Ok(request) => request,
            Err(_) => {
                let decided = fail_closed("app.journal", NOT_RECORDED, codes::JOURNAL_UNAVAILABLE);
                let id = ToolRequestId::new();
                self.log_decided(call.run_id, "unknown", &decided, id);
                return (
                    RulesOutcome {
                        decision: decided.decision(Some(call.agent_instance_id)),
                        tool_request_id: id,
                        approval_id: None,
                    },
                    decided.code,
                );
            }
        };
        let mut evaluated = self.evaluate(call, ctx, &digest.0, size).await;
        // D.10 The allowed call's counts, before the decision is journalled.
        if matches!(evaluated.decided.verdict, Verdict::Allow) {
            if let Some(facts) = evaluated.facts.as_mut() {
                let tool = bare(&call.tool).to_owned();
                let key = call_key(&tool, &call.arguments);
                *facts.tools_called.entry(key).or_insert(0) += 1;
                facts.tool_calls.push(tool.clone());
                if matches!(ctx.class, Some(ToolClass::LocalWrite)) || policy::is_write_tool(&tool)
                {
                    facts.changed_work_folder = true;
                }
                if self.save_run(facts).await.is_err() {
                    evaluated.decided =
                        fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE);
                }
            }
        }
        let mut decided = evaluated.decided;
        let deciding_rule: String = format!("{}:{}", source_text(decided.source), decided.rule_id)
            .chars()
            .take(128)
            .collect();
        if self
            .journal
            .decide_tool_request(
                request.id,
                decision_of(&decided),
                &deciding_rule,
                decided.approval_id,
            )
            .await
            .is_err()
        {
            decided = fail_closed("app.journal", NOT_RECORDED, codes::JOURNAL_UNAVAILABLE);
        }
        let tool_name = self.log_name(evaluated.set.as_deref(), &call.tool, &call.origin);
        self.log_decided(call.run_id, &tool_name, &decided, request.id);
        if let (Some(sink), Some(set), Some((family, control, source))) = (
            self.metrics(),
            evaluated.set.as_ref(),
            evaluated.metric.as_ref(),
        ) {
            sink.add(
                family,
                &[
                    ("agent", set.package_name.as_str()),
                    ("control", control.as_str()),
                    ("source", source_label(*source)),
                ],
                1,
            );
        }
        let code = decided.code;
        (
            RulesOutcome {
                decision: decided.decision(Some(call.agent_instance_id)),
                tool_request_id: request.id,
                approval_id: decided.approval_id,
            },
            code,
        )
    }

    fn log_name(&self, set: Option<&ControlSet>, tool: &str, origin: &ToolOrigin) -> String {
        let name = bare(tool);
        let known = policy::file_tool_argument(name).is_some()
            || policy::is_shell_tool(name)
            || name == "task"
            || matches!(origin, ToolOrigin::OrdinMcpFront { .. })
            || set.is_some_and(|s| s.is_bound(name));
        if known && name_shape().is_match(name) {
            name.to_owned()
        } else {
            "unknown".to_owned()
        }
    }

    fn log_decided(&self, run_id: RunId, tool: &str, decided: &Decided, id: ToolRequestId) {
        match decided.code {
            Some(code) => tracing::info!(
                run_id = run_id.to_string(),
                tool,
                decision = decided.kind(),
                source = source_text(decided.source),
                rule_id = decided.rule_id.as_str(),
                tool_request_id = id.to_string(),
                code,
                "rules_decided"
            ),
            None => tracing::info!(
                run_id = run_id.to_string(),
                tool,
                decision = decided.kind(),
                source = source_text(decided.source),
                rule_id = decided.rule_id.as_str(),
                tool_request_id = id.to_string(),
                "rules_decided"
            ),
        }
    }

    /// D.2–D.9.
    async fn evaluate(
        &self,
        call: &ToolCall,
        ctx: &CallContext,
        digest: &str,
        size: usize,
    ) -> Evaluated {
        // D.2 Input limits.
        if !(1..=MAX_TOOL_CHARS).contains(&call.tool.chars().count()) {
            return Evaluated::bare(fail_closed(
                "app.input.tool_name",
                MALFORMED,
                codes::TOOL_NAME_INVALID,
            ));
        }
        if size > MAX_ARGUMENT_BYTES {
            return Evaluated::bare(fail_closed(
                "app.input.arguments_size",
                MALFORMED,
                codes::ARGUMENTS_TOO_LARGE,
            ));
        }
        let server = match &call.origin {
            ToolOrigin::BuiltIn => None,
            ToolOrigin::OrdinMcpFront { server } | ToolOrigin::PackageMcpServer { server } => {
                Some(server.as_str())
            }
        };
        if let Some(server) = server {
            if !(1..=MAX_SERVER_CHARS).contains(&server.chars().count())
                || !server_shape().is_match(server)
            {
                return Evaluated::bare(fail_closed(
                    "app.input.origin",
                    MALFORMED,
                    codes::ORIGIN_INVALID,
                ));
            }
        }
        // D.3 Origin and name.
        let origin = self.classify(&call.origin);
        let tool = bare(&call.tool).to_owned();
        if !name_shape().is_match(&tool) {
            return Evaluated::bare(fail_closed(
                "app.input.tool_name",
                MALFORMED,
                codes::TOOL_NAME_INVALID,
            ));
        }
        // D.4 Run.
        let facts = match self.load_run(call.run_id).await {
            Ok(Some(facts)) => facts,
            Ok(None) => {
                return Evaluated::bare(fail_closed(
                    "app.run_unknown",
                    RUN_UNKNOWN,
                    codes::RUN_UNKNOWN,
                ))
            }
            Err(_) => {
                return Evaluated::bare(fail_closed(
                    "app.internal",
                    INTERNAL,
                    codes::STATE_UNAVAILABLE,
                ))
            }
        };
        if facts.agent_instance_id != call.agent_instance_id
            || facts.workspace_id != call.workspace_id
        {
            return Evaluated::bare(fail_closed(
                "app.session_mismatch",
                MISMATCH,
                codes::SESSION_MISMATCH,
            ));
        }
        let Some(set) = self.set_of(call.agent_instance_id) else {
            return Evaluated::bare(fail_closed(
                "app.ruleset",
                NOT_LOADED_REASON,
                codes::RULESET_NOT_LOADED,
            ));
        };
        let mut evaluated = Evaluated {
            decided: Decided::allow(RuleSource::AppApprovalPolicy, "app.default_allow"),
            facts: Some(facts),
            set: Some(set.clone()),
            metric: None,
        };
        let facts = evaluated
            .facts
            .as_ref()
            .cloned()
            .unwrap_or_else(|| unreachable!("set above"));
        if facts.ended || facts.single_call {
            let rule = facts
                .deciding_control
                .clone()
                .unwrap_or_else(|| "app.run_ended".to_owned());
            evaluated.decided =
                Decided::deny(RuleSource::DeclarationGate, &rule, NOT_OFFERED, None);
            return evaluated;
        }
        match self
            .checks(call, ctx, &origin, &tool, &facts, &set, digest)
            .await
        {
            Ok((decided, metric)) => {
                evaluated.decided = decided;
                evaluated.metric = metric;
            }
            Err(decided) => evaluated.decided = decided,
        }
        evaluated
    }

    /// D.3 for a caller outside a decision: the origin as this engine classifies it, so the Hook
    /// Relay's write claim treats a server the MCP Front serves as ordin's (HookRelay.md HR-ORIG-1).
    // spec: ordin-desktop/components/HookRelay#HR-ORIG-1
    pub fn classify_origin(&self, origin: &ToolOrigin) -> ToolOrigin {
        self.classify(origin)
    }

    /// D.3: a package server that the MCP Front serves is ordin's.
    // spec: ordin-desktop/components/RulesEngine#SA-RECON-08
    fn classify(&self, origin: &ToolOrigin) -> ToolOrigin {
        match origin {
            ToolOrigin::PackageMcpServer { server }
                if self.config.ordin_servers.iter().any(|s| s == server)
                    || self
                        .config
                        .ordin_server_prefixes
                        .iter()
                        .any(|p| server.starts_with(p.as_str())) =>
            {
                ToolOrigin::OrdinMcpFront {
                    server: server.clone(),
                }
            }
            other => other.clone(),
        }
    }

    /// D.5–D.9. `Err` carries a fail-closed denial.
    #[allow(clippy::too_many_arguments)]
    async fn checks(
        &self,
        call: &ToolCall,
        ctx: &CallContext,
        origin: &ToolOrigin,
        tool: &str,
        facts: &RunFacts,
        set: &Arc<ControlSet>,
        digest: &str,
    ) -> Result<
        (
            Decided,
            Option<(&'static str, String, crate::types::ControlSource)>,
        ),
        Decided,
    > {
        let profile = self.snapshot();
        let off: Vec<String> = profile
            .document
            .server
            .external_actions_off
            .iter()
            .filter_map(|k| serde_json::to_value(k).ok())
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        let personal = self.program() == ProgramKind::PersonalServer;
        let external_class = match &ctx.class {
            Some(ToolClass::External {
                kind,
                approval,
                destination,
                leaves_company,
            }) => Some((
                kind.as_str(),
                *approval,
                destination.as_str(),
                *leaves_company,
            )),
            _ => None,
        };
        // D.5.1
        if let Some((kind, ..)) = external_class {
            if personal && off.iter().any(|k| k == kind) {
                return Ok((
                    Decided::deny(
                        RuleSource::AppApprovalPolicy,
                        "app.server.external_off",
                        EXTERNAL_OFF,
                        None,
                    ),
                    None,
                ));
            }
        }
        // D.5.1b (ADR-102)
        // spec: ordin-desktop/components/RulesEngine#ADR-102
        if personal
            && off.iter().any(|k| k == "slack_post")
            && matches!(tool, "slack_draft_post" | "slack_ask_to_post")
            && matches!(origin, ToolOrigin::OrdinMcpFront { server } if server == "slack")
        {
            return Ok((
                Decided::deny(
                    RuleSource::AppApprovalPolicy,
                    "app.server.slack_off",
                    SLACK_OFF,
                    None,
                ),
                None,
            ));
        }
        // D.5.1a
        if let Some(restriction) = self.restriction_of(facts) {
            let outside = external_class.is_some()
                || set.external.iter().any(|t| t == tool)
                || matches!(origin, ToolOrigin::PackageMcpServer { .. });
            if !restriction.external_actions_allowed && outside {
                return Ok((
                    Decided::deny(
                        RuleSource::AppApprovalPolicy,
                        "app.crew.external_not_allowed",
                        CREW_OFF,
                        None,
                    ),
                    None,
                ));
            }
        }
        // D.5.2 Folder policy.
        if let Some(argument) = policy::file_tool_argument(tool) {
            let path = match text_arg(&call.arguments, argument) {
                Some(p) => Some(PathBuf::from(p)),
                None if policy::defaults_to_folder(tool)
                    && call.arguments.get(argument).is_none() =>
                {
                    Some(set.folder.clone())
                }
                None => None,
            };
            let allowed = path
                .and_then(|p| policy::absolute(Some(&set.folder), &p))
                .is_some_and(|p| {
                    self.path_allowed(
                        set,
                        Some(facts.workspace_id),
                        facts.crew_charter_id,
                        &p,
                        policy::is_write_tool(tool),
                    )
                });
            if !allowed {
                return Ok((
                    Decided::deny(
                        RuleSource::AppFolderPolicy,
                        "app.folder_grant",
                        FOLDER_REASON,
                        None,
                    ),
                    None,
                ));
            }
        }
        let shell = policy::is_shell_tool(tool);
        let command = if shell {
            text_arg(&call.arguments, "command")
                .unwrap_or_default()
                .to_owned()
        } else {
            String::new()
        };
        // D.5.3 Shell policy.
        if shell && !Self::is_system_changing(set) {
            return Ok((
                Decided::deny(RuleSource::AppShellPolicy, "app.shell", NO_COMMANDS, None),
                None,
            ));
        }
        // D.5.4 Agent tool policy.
        if shell {
            if let Some(hit) = policy::shell_violation(&command, &set.deny_verbs, &set.deny_flags) {
                return Ok((policy_deny(&hit), None));
            }
        } else {
            let key = call_key(tool, &call.arguments);
            if set.deny_verbs.iter().any(|v| names_the_call(v, tool, &key)) {
                return Ok((policy_deny(tool), None));
            }
        }
        // D.5.5 Tool subset.
        if let Some(subset) = &facts.subset {
            if set.is_bound(tool) && !subset.iter().any(|t| t == tool) {
                let rule = facts
                    .deciding_control
                    .clone()
                    .unwrap_or_else(|| "app.tool_subset".to_owned());
                return Ok((
                    Decided::deny(RuleSource::DeclarationGate, &rule, NOT_OFFERED, None),
                    None,
                ));
            }
        }
        // D.5.6 Reviewer (agent profile; C-78).
        // spec: ordin-desktop/components/RulesEngine#C-78
        if self.program().is_agent() {
            if let (Some(sub), Some(reviewer)) = (&ctx.subagent, &set.reviewer) {
                if sub == reviewer && !policy::READING_TOOLS.contains(&tool) {
                    return Ok((
                        Decided::deny(
                            RuleSource::DeclarationToolRule,
                            "app.reviewer_read_only",
                            REVIEWER,
                            None,
                        ),
                        None,
                    ));
                }
            }
        }
        // D.5.7 Step gates.
        let key = call_key(tool, &call.arguments);
        let mut evaluator = Evaluator {
            host: self.host.as_ref(),
            pool: self.datastore.sqlite(),
            set,
            facts,
            now_ms: now_ms(),
            cache: HashMap::new(),
            counts: HashMap::new(),
        };
        for (gate, source) in set
            .steps
            .iter()
            .filter(|(g, _)| g.point == Point::BeforeTool)
        {
            let command_applies = match &gate.command {
                Some(c) => facts.commands.contains(c),
                None => facts.commands.is_empty(),
            };
            let tool_applies = gate
                .tool
                .as_deref()
                .is_none_or(|t| names_the_call(t, tool, &key));
            if !(command_applies && tool_applies) {
                continue;
            }
            match evaluator
                .all(&gate.conditions, facts.commands.first().map(String::as_str))
                .await
            {
                Ok(true) => {
                    let why = format!(
                        "Refused by the control {}: the declared condition holds.",
                        gate.name
                    );
                    return Ok((
                        Decided::deny(RuleSource::DeclarationGate, &gate.name, &why, None),
                        Some((
                            "wrightrun_tool_calls_refused_total",
                            gate.name.clone(),
                            *source,
                        )),
                    ));
                }
                Ok(false) => {}
                Err(error) => return Err(self.fact_denial(&gate.name, tool, &error)),
            }
        }
        // D.5.8 Argument rules with outcome refuse.
        let mut package_unless_passed = false;
        for (rule, source) in set
            .argument_rules
            .iter()
            .filter(|(r, _)| names_the_call(&r.tool, tool, &key))
            .filter(|(r, _)| r.outcome == ArgumentOutcome::Refuse)
        {
            if args::holds(rule, &call.arguments, &set.patterns) {
                let why = format!(
                    "Refused by the control {}: the argument {} is not permitted.",
                    rule.name, rule.argument
                );
                return Ok((
                    Decided::deny(RuleSource::DeclarationArgumentRule, &rule.name, &why, None),
                    Some((
                        "wrightrun_tool_calls_refused_total",
                        rule.name.clone(),
                        *source,
                    )),
                ));
            }
            if rule.form == ArgumentForm::RefuseUnless
                && *source == crate::types::ControlSource::Package
            {
                package_unless_passed = true;
            }
        }
        // D.5.9 Email profile rules.
        let mail_tool = matches!(tool, "email_draft" | "email_ask_to_send")
            && matches!(origin, ToolOrigin::OrdinMcpFront { server } if server == "mail");
        if mail_tool || matches!(external_class, Some(("email_send", ..))) {
            if let Some(denied) = self.email_rules(&call.arguments) {
                return Ok((denied, None));
            }
        }
        // D.6 Approved once (C-80).
        // spec: ordin-desktop/components/RulesEngine#C-80
        let pool = self
            .pool()
            .map_err(|_| fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE))?;
        let run = facts.run_id.to_string();
        let mut reuse: Option<(ApprovalId, ActionProposal)> = None;
        if let Some(approval) = store::open_approved_call(pool, &run, &call.tool, digest)
            .await
            .map_err(|_| fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE))?
        {
            match self.approved_once(&approval).await? {
                Once::Allow => {
                    return Ok((
                        Decided::allow(RuleSource::AppApprovalPolicy, "app.approved_once"),
                        None,
                    ))
                }
                Once::Deny => {
                    return Ok((
                        Decided::deny(
                            RuleSource::AppApprovalPolicy,
                            "app.approved_once",
                            REJECTED,
                            None,
                        ),
                        None,
                    ))
                }
                Once::Pending(id, proposal) => reuse = Some((id, proposal)),
                Once::Gone => {}
            }
        }
        // D.7 Ask checks.
        drop(evaluator);
        let asking = self.ask_checks(
            call,
            origin,
            tool,
            &command,
            facts,
            set,
            package_unless_passed,
            external_class,
        );
        let Some(asking) = asking else {
            if let Some((id, proposal)) = reuse {
                return Ok((
                    ask_decided(
                        RuleSource::AppApprovalPolicy,
                        "app.approved_once",
                        id,
                        proposal,
                    ),
                    None,
                ));
            }
            // D.9 Allow.
            let source = if policy::file_tool_argument(tool).is_some() {
                RuleSource::AppFolderPolicy
            } else {
                RuleSource::AppApprovalPolicy
            };
            let rule = if asking_git_allowed(tool, &command, shell, self, facts, set) {
                "app.git_host_setting"
            } else {
                "app.default_allow"
            };
            return Ok((Decided::allow(source, rule), None));
        };
        let metric = asking.rule_source.map(|s| {
            (
                "wrightrun_tool_calls_paused_by_rule_total",
                asking.rule_id.clone(),
                s,
            )
        });
        if let Some((id, proposal)) = reuse {
            return Ok((
                ask_decided(asking.source, &asking.rule_id, id, proposal),
                metric,
            ));
        }
        // D.8 Ask.
        let decided = self.open_ask(facts, &call.tool, digest, asking).await;
        Ok((decided, metric))
    }

    fn fact_denial(&self, control: &str, tool: &str, error: &EvalError) -> Decided {
        let code = match error {
            EvalError::Fact => codes::FACT_UNAVAILABLE,
            EvalError::State => codes::STATE_UNAVAILABLE,
        };
        tracing::warn!(
            control,
            tool = if name_shape().is_match(tool) {
                tool
            } else {
                "unknown"
            },
            error_code = code,
            "rules_fact_unavailable"
        );
        let why =
            format!("Refused by the control {control}: a declared control could not be evaluated.");
        Decided::deny(RuleSource::FailClosed, control, &why, Some(code))
    }

    /// D.5.9 over the mail tools' arguments.
    fn email_rules(&self, arguments: &Value) -> Option<Decided> {
        let unreadable = || {
            Decided::deny(
                RuleSource::AppApprovalPolicy,
                "app.email.max_recipients",
                "Refused by the organisation's email rules: the recipients could not be read.",
                None,
            )
        };
        let addresses = |name: &str| -> Option<Vec<String>> {
            match arguments.get(name) {
                None | Some(Value::Null) => Some(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        item.get("address")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .collect(),
                Some(_) => None,
            }
        };
        let (Some(to), Some(cc), Some(bcc)) = (addresses("to"), addresses("cc"), addresses("bcc"))
        else {
            return Some(unreadable());
        };
        let attachments: Option<Vec<ordin_types::domain::email::EmailAttachment>> =
            match arguments.get("attachments") {
                None | Some(Value::Null) => Some(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| {
                        let place = item
                            .get("place_id")
                            .and_then(Value::as_str)
                            .and_then(|p| PlaceId::parse(p).ok())?;
                        let path = item.get("relative_path").and_then(Value::as_str)?;
                        Some(ordin_types::domain::email::EmailAttachment {
                            place_id: place,
                            path: path.to_owned(),
                        })
                    })
                    .collect(),
                Some(_) => None,
            };
        let Some(attachments) = attachments else {
            return Some(unreadable());
        };
        let verdict = self.check_email(&EmailCheck {
            to: &to,
            cc: &cc,
            bcc: &bcc,
            attachments: &attachments,
            drafted_by_agent: true,
            body: "",
            foot_line: "",
        });
        let EmailVerdict::Refused { reasons } = verdict else {
            return None;
        };
        let rule = match reasons.first() {
            Some(EmailViolation::EmailOff) | None => "app.email.off",
            Some(EmailViolation::TooManyRecipients { .. }) => "app.email.max_recipients",
            Some(EmailViolation::RecipientOutsideCompany { .. }) => "app.email.internal_only",
            Some(EmailViolation::AttachmentFromClosedPlace { .. }) => "app.email.no_attach",
            Some(EmailViolation::FootLineMissing) => "app.email.foot_line",
        };
        let why = format!("Refused by the organisation's email rules: {rule}.");
        Some(Decided::deny(
            RuleSource::AppApprovalPolicy,
            rule,
            &why,
            None,
        ))
    }

    async fn approved_once(&self, approval: &str) -> Result<Once, Decided> {
        let unavailable =
            || fail_closed("app.approval", APPROVAL_DOWN, codes::APPROVAL_UNAVAILABLE);
        let id = ApprovalId::parse(approval)
            .map_err(|_| fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE))?;
        let record = self.approvals.get(id).await.map_err(|_| unavailable())?;
        let pool = self
            .pool()
            .map_err(|_| fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE))?;
        let state_error = |_| fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE);
        Ok(match record.state {
            ApprovalState::Approved => {
                store::mark_used(pool, approval)
                    .await
                    .map_err(state_error)?;
                Once::Allow
            }
            ApprovalState::Pending => Once::Pending(id, record.proposal),
            ApprovalState::Rejected => Once::Deny,
            ApprovalState::Expired => {
                store::delete_approved_call(pool, approval)
                    .await
                    .map_err(state_error)?;
                Once::Gone
            }
        })
    }

    /// D.7 in order; the first that holds asks.
    #[allow(clippy::too_many_arguments)]
    fn ask_checks(
        &self,
        call: &ToolCall,
        origin: &ToolOrigin,
        tool: &str,
        command: &str,
        facts: &RunFacts,
        set: &ControlSet,
        package_unless_passed: bool,
        external_class: Option<(&str, ApprovalNeed, &str, bool)>,
    ) -> Option<Asking> {
        let in_external = set.external.iter().any(|t| t == tool);
        let leaves = external_class.is_some_and(|(.., l)| l) || in_external;
        let ask = |source, rule_id: &str, proposal| Asking {
            source,
            rule_id: rule_id.to_owned(),
            proposal,
            content: None,
            leaves_company: leaves,
            rule_source: None,
        };
        // D.7.1 Argument rules with outcome pause.
        for (rule, source) in set
            .argument_rules
            .iter()
            .filter(|(r, _)| names_the_call(&r.tool, tool, &call_key(tool, &call.arguments)))
            .filter(|(r, _)| r.outcome == ArgumentOutcome::Pause)
        {
            if args::holds(rule, &call.arguments, &set.patterns) {
                let mut asking = ask(
                    RuleSource::DeclarationArgumentRule,
                    &rule.name,
                    Proposal::ArgumentRulePause {
                        rule: rule.name.clone(),
                        tool: call.tool.clone(),
                    },
                );
                asking.rule_source = Some(*source);
                return Some(asking);
            }
        }
        // D.7.2 Approval list.
        let key = call_key(tool, &call.arguments);
        let listed = set.approval_list.iter().any(|entry| {
            let entry = bare(entry);
            names_the_call(entry, tool, &key)
        });
        if listed {
            return Some(ask(
                RuleSource::DeclarationToolRule,
                "approval-list",
                Proposal::ApprovalListTool {
                    tool: call.tool.clone(),
                },
            ));
        }
        // D.7.3 Agent policy approval.
        let shell = policy::is_shell_tool(tool);
        let verb_hit = if shell {
            policy::verbs(command)
                .into_iter()
                .find_map(|(verb, program)| {
                    set.ask_verbs
                        .iter()
                        .find(|v| **v == verb || **v == program)
                        .cloned()
                })
        } else {
            let key = call_key(tool, &call.arguments);
            set.ask_verbs
                .iter()
                .find(|v| names_the_call(v, tool, &key))
                .cloned()
        };
        if let Some(verb) = verb_hit {
            let mut asking = ask(
                RuleSource::AgentToolPolicy,
                &verb,
                Proposal::AgentPolicyApproval {
                    verb: verb.clone(),
                    tool: call.tool.clone(),
                },
            );
            if shell {
                asking.content = Some((command.to_owned(), set.folder.display().to_string()));
            }
            return Some(asking);
        }
        // D.7.4 Shell (SA-RULE-02, K.3).
        // spec: ordin-desktop/components/RulesEngine#SA-RULE-02
        if shell
            && Self::is_system_changing(set)
            && !(package_unless_passed && policy::one_plain_command(tool, command))
            && !self.git_allows(tool, command, facts, set, None)
        {
            let mut asking = ask(
                RuleSource::AppShellPolicy,
                "app.shell",
                Proposal::ShellCommand {
                    command: command.to_owned(),
                    runs_in: set.folder.display().to_string(),
                },
            );
            asking.content = Some((command.to_owned(), set.folder.display().to_string()));
            return Some(asking);
        }
        // D.7.5 External action.
        let always = matches!(external_class, Some((_, ApprovalNeed::Always, ..)));
        if always || in_external {
            let (kind, destination) = match external_class {
                Some((kind, _, destination, _)) => (kind.to_owned(), destination.to_owned()),
                None => ("connection_write".to_owned(), String::new()),
            };
            return Some(ask(
                RuleSource::AppApprovalPolicy,
                "app.external_action",
                Proposal::ExternalTool {
                    tool: call.tool.clone(),
                    kind,
                    destination,
                },
            ));
        }
        // D.7.6 Package tool.
        // spec: ordin-desktop/components/RulesEngine#SA-RULE-03
        if let ToolOrigin::PackageMcpServer { server } = origin {
            if !set.package_read_only_tools.iter().any(|t| t == tool) {
                return Some(ask(
                    RuleSource::AppApprovalPolicy,
                    "app.package_tool",
                    Proposal::PackageTool {
                        server: server.clone(),
                        tool: call.tool.clone(),
                    },
                ));
            }
        }
        None
    }

    /// K.3: every segment of the command is a GitHub or GitLab kind that needs no approval.
    /// `working_dir` Some = `decide_command`'s project; None = the run's one granted project place.
    pub(crate) fn git_allows(
        &self,
        tool: &str,
        command: &str,
        facts: &RunFacts,
        set: &ControlSet,
        working_dir: Option<&Path>,
    ) -> bool {
        let segments = policy::segments(command);
        if segments.is_empty() || !policy::reads_plainly(tool, command) {
            return false;
        }
        let settings = read(&self.git_settings);
        let project: Option<Vec<crate::types::GitHostSetting>> = match working_dir {
            Some(dir) => {
                let grants = read(&self.grants);
                grants
                    .values()
                    .flatten()
                    .filter(|g| policy::is_under(dir, &g.root))
                    .find_map(|g| settings.get(&g.place_id).cloned())
            }
            None => {
                let grants = read(&self.grants);
                let places: Vec<PlaceId> = grants
                    .get(&facts.workspace_id)
                    .map(|gs| {
                        gs.iter()
                            .filter(|g| match g.grantee {
                                Grantee::AgentInstance { agent_instance_id } => {
                                    agent_instance_id == set.id
                                }
                                Grantee::Crew { crew_charter_id } => {
                                    Some(crew_charter_id) == facts.crew_charter_id
                                }
                            })
                            .filter(|g| settings.contains_key(&g.place_id))
                            .map(|g| g.place_id)
                            .collect()
                    })
                    .unwrap_or_default();
                match places.as_slice() {
                    [one] => settings.get(one).cloned(),
                    _ => None,
                }
            }
        };
        segments
            .iter()
            .all(|segment| match policy::git_host_kind(segment) {
                None => false,
                Some("read") => true,
                Some(kind) => project.as_ref().is_some_and(|list| {
                    list.iter().any(|s| s.kind == kind && !s.approval_required)
                }),
            })
    }

    /// D.8: opens one approval through the Approval Service and records the approved-once row.
    async fn open_ask(
        &self,
        facts: &RunFacts,
        tool: &str,
        digest: &str,
        asking: Asking,
    ) -> Decided {
        let wording = self.host.word(&facts.key(), &asking.proposal);
        let content = asking
            .content
            .as_ref()
            .map(|(command, runs_in)| ApprovalContent::Command {
                label: wording
                    .content_label
                    .clone()
                    .unwrap_or_else(|| SEE_THE_COMMAND.to_owned()),
                command: command.clone(),
                runs_in: runs_in.clone(),
            });
        let request = ApprovalRequest {
            workspace_id: facts.workspace_id,
            asked_by: ApprovalAsker::Run {
                run_id: facts.run_id,
            },
            question: wording.question,
            verb: wording.verb,
            where_line: wording.where_line,
            affects: wording.affects,
            changes: wording.changes,
            show_details: content.is_some(),
            content,
            reject_style: RejectStyle::Reject,
            proposal: wording.proposal.clone(),
            asker: facts.asker.clone(),
            leaves_company: asking.leaves_company,
        };
        let card = match self.approvals.request(request).await {
            Ok(card) => card,
            Err(_) => {
                return fail_closed("app.approval", APPROVAL_DOWN, codes::APPROVAL_UNAVAILABLE)
            }
        };
        let recorded = match self.pool() {
            Ok(pool) => store::insert_approved_call(
                pool,
                &facts.run_id.to_string(),
                tool,
                digest,
                &card.approval_id.to_string(),
                now_ms(),
            )
            .await
            .is_ok(),
            Err(_) => false,
        };
        if !recorded {
            return fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE);
        }
        ask_decided(
            asking.source,
            &asking.rule_id,
            card.approval_id,
            wording.proposal,
        )
    }

    /// D2. A Journal error is logged and not returned.
    pub async fn record_outcome(&self, tool_request_id: ToolRequestId, executed: bool) {
        if let Err(error) = self
            .journal
            .finish_tool_request(tool_request_id, executed)
            .await
        {
            tracing::warn!(
                tool_request_id = tool_request_id.to_string(),
                error_code = ordin_types::CodedError::code(&error),
                "rules_outcome_unrecorded"
            );
        }
    }

    /// K.2.
    pub async fn wait_for_approval(
        &self,
        approval_id: ApprovalId,
        until: Option<tokio::time::Instant>,
    ) -> Result<ApprovalWait, RulesError> {
        let outcome = self
            .approvals
            .wait(approval_id, until)
            .await
            .map_err(|_| RulesError::ApprovalUnavailable)?;
        Ok(match outcome {
            WaitOutcome::Decided(ApprovalState::Approved) => {
                store::mark_used(self.pool()?, &approval_id.to_string()).await?;
                ApprovalWait::Approved
            }
            WaitOutcome::Decided(ApprovalState::Rejected) => ApprovalWait::Rejected,
            WaitOutcome::Decided(ApprovalState::Expired) => ApprovalWait::Expired,
            WaitOutcome::Decided(ApprovalState::Pending) | WaitOutcome::DeadlinePassed => {
                ApprovalWait::Pending
            }
        })
    }

    /// K.1. Never fails; every outcome is a `RulesOutcome`.
    pub async fn decide_command(&self, req: CommandRequest) -> RulesOutcome {
        self.decide_command_coded(req).await.0
    }

    pub(crate) async fn decide_command_coded(
        &self,
        req: CommandRequest,
    ) -> (RulesOutcome, Option<&'static str>) {
        // K.1.1
        let chars = req.command.chars().count();
        let invalid = if !(1..=MAX_COMMAND_CHARS).contains(&chars) || !req.working_dir.is_absolute()
        {
            Some(fail_closed(
                "app.input.command",
                MALFORMED,
                codes::COMMAND_INVALID,
            ))
        } else if policy::absolute(None, &req.working_dir).is_some_and(|dir| self.protected(&dir)) {
            Some(Decided::deny(
                RuleSource::AppFolderPolicy,
                "app.folder_grant",
                FOLDER_REASON,
                Some(codes::PATH_NOT_ALLOWED),
            ))
        } else {
            None
        };
        let digest = digest_of(&serde_json::json!({
            "command": req.command,
            "working_dir": req.working_dir.display().to_string(),
        }))
        .0;
        match &req.origin {
            CommandOrigin::Person => {
                // K.1.2 / K.1.3
                let decided = invalid.unwrap_or_else(|| {
                    if req.declared_check {
                        Decided::allow(RuleSource::AppShellPolicy, "app.shell.person_check")
                    } else {
                        Decided::deny(
                            RuleSource::AppShellPolicy,
                            "app.shell.person_undeclared",
                            PERSON_UNDECLARED,
                            Some(codes::COMMAND_NOT_DECLARED),
                        )
                    }
                });
                // spec: ordin-desktop/components/RulesEngine#SA-RECON-06
                let recorded = self
                    .journal_entry(
                        "rules_command_decided",
                        None,
                        Some(req.workspace_id),
                        "The rules engine decided a command a person asked for.",
                        serde_json::json!({
                            "workspace_id": req.workspace_id.to_string(),
                            "origin": "person",
                            "decision": decided.kind(),
                            "rule": decided.rule_id,
                        }),
                    )
                    .await;
                let decided = if recorded {
                    decided
                } else {
                    fail_closed("app.journal", NOT_RECORDED, codes::JOURNAL_UNAVAILABLE)
                };
                log_command("person", &decided);
                let code = decided.code;
                (
                    RulesOutcome {
                        decision: decided.decision(None),
                        tool_request_id: ToolRequestId::new(),
                        approval_id: None,
                    },
                    code,
                )
            }
            CommandOrigin::Run {
                run_id,
                agent_instance_id,
            } => {
                // K.1.2
                // spec: ordin-desktop/components/RulesEngine#SA-RECON-07
                let request = match self
                    .journal
                    .record_tool_request(*run_id, COMMAND_TOOL, digest.clone())
                    .await
                {
                    Ok(request) => request,
                    Err(_) => {
                        let decided =
                            fail_closed("app.journal", NOT_RECORDED, codes::JOURNAL_UNAVAILABLE);
                        log_command("run", &decided);
                        return (
                            RulesOutcome {
                                decision: decided.decision(Some(*agent_instance_id)),
                                tool_request_id: ToolRequestId::new(),
                                approval_id: None,
                            },
                            decided.code,
                        );
                    }
                };
                let mut decided = match invalid {
                    Some(decided) => decided,
                    None => {
                        self.command_for_run(&req, *run_id, *agent_instance_id, &digest.0)
                            .await
                    }
                };
                let rule: String = format!("{}:{}", source_text(decided.source), decided.rule_id)
                    .chars()
                    .take(128)
                    .collect();
                if self
                    .journal
                    .decide_tool_request(
                        request.id,
                        decision_of(&decided),
                        &rule,
                        decided.approval_id,
                    )
                    .await
                    .is_err()
                {
                    decided = fail_closed("app.journal", NOT_RECORDED, codes::JOURNAL_UNAVAILABLE);
                }
                log_command("run", &decided);
                let code = decided.code;
                (
                    RulesOutcome {
                        decision: decided.decision(Some(*agent_instance_id)),
                        tool_request_id: request.id,
                        approval_id: decided.approval_id,
                    },
                    code,
                )
            }
        }
    }

    /// K.1.4.
    async fn command_for_run(
        &self,
        req: &CommandRequest,
        run_id: RunId,
        agent: ordin_types::AgentInstanceId,
        digest: &str,
    ) -> Decided {
        let facts = match self.load_run(run_id).await {
            Ok(Some(facts)) => facts,
            Ok(None) => return fail_closed("app.run_unknown", RUN_UNKNOWN, codes::RUN_UNKNOWN),
            Err(_) => return fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE),
        };
        if facts.agent_instance_id != agent || facts.workspace_id != req.workspace_id {
            return fail_closed("app.session_mismatch", MISMATCH, codes::SESSION_MISMATCH);
        }
        let Some(set) = self.set_of(agent) else {
            return fail_closed("app.ruleset", NOT_LOADED_REASON, codes::RULESET_NOT_LOADED);
        };
        if !Self::is_system_changing(&set) {
            return Decided::deny(RuleSource::AppShellPolicy, "app.shell", NO_COMMANDS, None);
        }
        if let Some(hit) = policy::shell_violation(&req.command, &set.deny_verbs, &set.deny_flags) {
            return policy_deny(&hit);
        }
        let pool = match self.pool() {
            Ok(pool) => pool,
            Err(_) => return fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE),
        };
        let mut reuse = None;
        match store::open_approved_call(pool, &run_id.to_string(), COMMAND_TOOL, digest).await {
            Err(_) => return fail_closed("app.internal", INTERNAL, codes::STATE_UNAVAILABLE),
            Ok(None) => {}
            Ok(Some(approval)) => match self.approved_once(&approval).await {
                Err(decided) => return decided,
                Ok(Once::Allow) => {
                    return Decided::allow(RuleSource::AppApprovalPolicy, "app.approved_once")
                }
                Ok(Once::Deny) => {
                    return Decided::deny(
                        RuleSource::AppApprovalPolicy,
                        "app.approved_once",
                        REJECTED,
                        None,
                    )
                }
                Ok(Once::Pending(id, proposal)) => reuse = Some((id, proposal)),
                Ok(Once::Gone) => {}
            },
        }
        if req.declared_check {
            return Decided::allow(RuleSource::AppShellPolicy, "app.shell.declared_check");
        }
        if self.git_allows(
            COMMAND_TOOL,
            &req.command,
            &facts,
            &set,
            Some(&req.working_dir),
        ) {
            return Decided::allow(RuleSource::AppApprovalPolicy, "app.git_host_setting");
        }
        let runs_in = req.working_dir.display().to_string();
        let verb_hit = policy::verbs(&req.command)
            .into_iter()
            .find_map(|(verb, program)| {
                set.ask_verbs
                    .iter()
                    .find(|v| **v == verb || **v == program)
                    .cloned()
            });
        let asking = match verb_hit {
            Some(verb) => Asking {
                source: RuleSource::AgentToolPolicy,
                rule_id: verb.clone(),
                proposal: Proposal::AgentPolicyApproval {
                    verb,
                    tool: COMMAND_TOOL.to_owned(),
                },
                content: Some((req.command.clone(), runs_in.clone())),
                leaves_company: false,
                rule_source: None,
            },
            None => Asking {
                source: RuleSource::AppShellPolicy,
                rule_id: "app.shell".to_owned(),
                proposal: Proposal::ShellCommand {
                    command: req.command.clone(),
                    runs_in: runs_in.clone(),
                },
                content: Some((req.command.clone(), runs_in)),
                leaves_company: false,
                rule_source: None,
            },
        };
        if let Some((id, proposal)) = reuse {
            return ask_decided(asking.source, &asking.rule_id, id, proposal);
        }
        self.open_ask(&facts, COMMAND_TOOL, digest, asking).await
    }
}

enum Once {
    Allow,
    Deny,
    Pending(ApprovalId, ActionProposal),
    Gone,
}

fn ask_decided(
    source: RuleSource,
    rule_id: &str,
    approval: ApprovalId,
    proposal: ActionProposal,
) -> Decided {
    Decided {
        verdict: Verdict::Ask { proposal },
        source,
        rule_id: rule_id.to_owned(),
        approval_id: Some(approval),
        code: None,
    }
}

fn policy_deny(hit: &str) -> Decided {
    let why = format!("Refused by the agent's tool policy: {hit} is not allowed.");
    Decided::deny(RuleSource::AgentToolPolicy, hit, &why, None)
}

/// Whether a shell call allowed by D.9 was allowed because of K.3 (for the rule it names).
fn asking_git_allowed(
    tool: &str,
    command: &str,
    shell: bool,
    engine: &RulesEngine,
    facts: &RunFacts,
    set: &ControlSet,
) -> bool {
    shell
        && RulesEngine::is_system_changing(set)
        && engine.git_allows(tool, command, facts, set, None)
}

fn log_command(origin: &str, decided: &Decided) {
    match decided.code {
        Some(code) => tracing::info!(
            origin,
            decision = decided.kind(),
            rule_id = decided.rule_id.as_str(),
            code,
            "rules_command_decided"
        ),
        None => tracing::info!(
            origin,
            decision = decided.kind(),
            rule_id = decided.rule_id.as_str(),
            "rules_command_decided"
        ),
    }
}

/// Whether a control's tool name covers this call: the tool, its key, `task` for any sub-agent, or
/// the sub-agent Claude Code runs when the call names none.
fn names_the_call(entry: &str, tool: &str, key: &str) -> bool {
    entry == tool
        || entry == key
        || (entry == "task" && key.starts_with("task:"))
        || (entry == DEFAULT_SUB_AGENT && key == "task")
}

/// The sub-agent of a `task` call with no `subagent_type`.
pub(crate) const DEFAULT_SUB_AGENT: &str = "task:general-purpose";

/// The call's key: `task:<subagent_type>` for the `task` tool, else the bare tool name. Claude Code
/// 2.1.316 lists the tool as `Task` and names it `Agent` in the hook payload: both are the same tool.
// spec: ordin-desktop/components/RulesEngine#SA-RULE-01
pub(crate) fn call_key(tool: &str, arguments: &Value) -> String {
    if tool.eq_ignore_ascii_case("task") || tool == "Agent" {
        match arguments.get("subagent_type").and_then(Value::as_str) {
            Some(sub) => format!("task:{sub}"),
            None => "task".to_owned(),
        }
    } else {
        tool.to_owned()
    }
}
