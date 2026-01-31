//! X2: one test per Gherkin scenario of Checker.md (with its January 2026 amendment), under the
//! scenario's name in snake_case. The parity scenario is `tests/parity.rs`, an integration test
//! over the committed fixtures in `testdata/parity/checks/`.

use std::collections::BTreeMap;
use std::sync::Arc;

use ordin_agent_model::{
    ModelPart, PartKind, PartSource, Resolution, SourceOverride, SourceScope, SourcedPart,
    Timestamp, WithdrawnListEntry,
};
use ordin_resolver::compile_instructions;
use ordin_types::domain::agents::{AgentDraft, ExecutablePartKind, ExecutablePartTrust};
use ordin_types::{AgentInstanceId, PartDraftId, WithdrawnListEntryId};
use serde_json::json;

use super::support::*;
use crate::context::resolve_request;
use crate::model::*;
use crate::CheckError;

fn draft(name: &str, purpose: &str, jobs: &[&str], tasks: &[&str]) -> AgentDraft {
    let now = Timestamp::now().to_wire();
    serde_json::from_value(json!({
        "id": uuid::Uuid::now_v7().to_string(),
        "agent_repository_id": "01900000-0000-7000-8000-0000000000a1",
        "name": name,
        "purpose": purpose,
        "jobs": jobs.iter().map(|j| json!({"text": j, "came_from": "maker"})).collect::<Vec<_>>(),
        "tasks": tasks,
        "will": [], "wont": [], "needs": [], "reused_parts": [],
        "origin": "made",
        "branch": "ordin/draft/contract-clause-checker",
        "wording_changes": [], "field_authors": [],
        "trial": false,
        "state": "draft",
        "created_at": now, "updated_at": now,
    }))
    .expect("an agent draft")
}

const JOB: &str = "Flag clauses that differ from the template";
const TASK: &str = "Compare clauses with the template";

/// A draft check of `parts` against a repository holding `home`.
pub(super) fn draft_check(draft: AgentDraft, parts: Vec<ModelPart>) -> AgentDraftCheck {
    let mut job_tasks = BTreeMap::new();
    for i in 0..draft.jobs.len() {
        job_tasks.insert(u32::try_from(i).unwrap(), vec![TASK.to_owned()]);
    }
    AgentDraftCheck {
        draft,
        parts: parts.into_iter().map(staged).collect(),
        target: repository_check(Vec::new()),
        target_format: TargetFormat::Ordin,
        job_tasks,
        mapping_report: Vec::new(),
        loss_report: Vec::new(),
        borrowed: Vec::new(),
        repository_agent_names: vec!["Supplier analyst".to_owned()],
        rehearsal: None,
        agent_language: "en-GB".to_owned(),
        reachable_places: BTreeMap::new(),
    }
}

pub(super) fn clean_draft() -> AgentDraft {
    draft(
        "Contract clause checker",
        "Analyses contract clauses",
        &[JOB],
        &[TASK],
    )
}

fn jira() -> ModelPart {
    tool_binding("jira", &mcp("jira-reader"))
}

fn agent_with_tools(slug: &str) -> ModelPart {
    workspace(
        slug,
        &format!(
            "{}toolBindings:\n  - urn:ordin:acme:tool-bindings:jira\n",
            minimal_body()
        ),
    )
}

#[tokio::test]
async fn happy_path_a_clean_draft_passes_and_the_wording_is_tidied() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let agent = workspace("contract-clause-checker", &minimal_body());
    let check = draft_check(
        draft(
            "Contract clause checker",
            "Analyzes contract clauses",
            &[JOB],
            &[TASK],
        ),
        vec![agent],
    );
    let report = f.checker.check_agent_draft(&check).await.unwrap();
    assert!(report.passed, "findings: {:?}", report.findings);
    assert!(report.tidied.iter().any(|c| c.field == "purpose"
        && c.before == "Analyzes contract clauses"
        && c.after == "Analyses contract clauses"));
    assert!(report.publish_gate_met);
    // The run is recorded and can be read back.
    let again = f.checker.report(report.check_run_id).await.unwrap();
    assert_eq!(again.findings, report.findings);
    assert_eq!(again.tidied, report.tidied);
    assert_eq!(f.journal_kinds().await, vec!["check_run_completed"]);
}

#[tokio::test]
async fn error_tool_outside_the_trust_policy() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    f.trust.set(json!({ "tool_bindings": [] }));
    let check = draft_check(
        clean_draft(),
        vec![agent_with_tools("clause-checker"), jira()],
    );
    let report = f.checker.check_agent_draft(&check).await.unwrap();
    let found = named(&report.findings, "trust_needs_it_approval");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(found[0].severity, Severity::Failure);
    assert_eq!(found[0].args["tool"], "jira-reader");
    assert_eq!(
        found[0].fixes,
        vec![
            Fix::LeaveItOut {
                what: "jira-reader".to_owned()
            },
            Fix::AskIt
        ]
    );
    assert!(!report.passed);
}

fn testing_pack(slug: &str, skill: &str) -> ModelPart {
    part(
        &format!("models/@acme/practice-packs/testing/{slug}/practice-pack.yaml"),
        &yaml(
            "practice-pack",
            &format!("urn:ordin:acme:practice-packs:testing:{slug}"),
            slug,
            &format!("{DESCRIPTIVE}  class: testing\n"),
            &format!("skills:\n  - {skill}\n"),
        ),
    )
}

fn skill(publisher: &str, slug: &str, version: &str, body: &str) -> ModelPart {
    part(
        &format!("models/@{publisher}/skills/{slug}/SKILL.md"),
        &format!(
            "---\napiVersion: ai.enterprise/v1\nkind: skill\nmetadata:\n  id: urn:ordin:{publisher}:skills:{slug}\n  name: {slug}\n  version: {version}\n  owner: legal-ops\n---\n{body}"
        ),
    )
}

#[tokio::test]
async fn error_two_parts_disagree() {
    let mut home = base();
    home.push(testing_pack(
        "pyramid",
        "urn:ordin:alpha:skills:house-style",
    ));
    home.push(testing_pack("trophy", "urn:ordin:beta:skills:house-style"));
    home.push(skill("alpha", "house-style", "1.0.0", "Terse.\n"));
    home.push(skill("beta", "house-style", "2.0.0", "Verbose.\n"));
    let f = Fixture::with(source_of(home, Vec::new())).await;
    let agent = workspace(
        "clause-checker",
        &format!(
            "{}requiredPracticePacks:\n  - atLeast: 2\n    where:\n      class: testing\nfixedPracticePacks:\n  - urn:ordin:acme:practice-packs:testing:pyramid\n  - urn:ordin:acme:practice-packs:testing:trophy\n",
            minimal_body()
        ),
    );
    let report = f
        .checker
        .check_agent_draft(&draft_check(clean_draft(), vec![agent]))
        .await
        .unwrap();
    let found = named(&report.findings, "parts_conflict");
    assert!(!found.is_empty(), "findings: {:?}", report.findings);
    assert_eq!(found[0].severity, Severity::Failure);
    assert_eq!(found[0].fixes, vec![Fix::AskAgentMaker]);
    assert_eq!(found[0].message_id, "check-parts-disagree");
    assert_eq!(found[0].args["what"], "house-style");
    assert!(!report.passed);
}

fn stack_pack(slug: &str) -> ModelPart {
    part(
        &format!("models/@acme/practice-packs/stack/{slug}/practice-pack.yaml"),
        &yaml(
            "practice-pack",
            &format!("urn:ordin:acme:practice-packs:stack:{slug}"),
            slug,
            &format!("{DESCRIPTIVE}  class: stack\n"),
            "",
        ),
    )
}

#[tokio::test]
async fn error_the_makers_fixed_packs_do_not_settle_a_required_rule() {
    let mut home = base();
    home.push(stack_pack("go"));
    home.push(stack_pack("rust"));
    let f = Fixture::with(source_of(home, Vec::new())).await;
    let agent = workspace(
        "clause-checker",
        &format!(
            "{}requiredPracticePacks:\n  - atLeast: 1\n    where:\n      class: stack\n",
            minimal_body()
        ),
    );
    let report = f
        .checker
        .check_agent_draft(&draft_check(clean_draft(), vec![agent]))
        .await
        .unwrap();
    let found = named(&report.findings, "fixed_packs_unsatisfied");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(found[0].args["class"], "technology stack");
    assert_eq!(
        found[0].fixes,
        vec![
            Fix::ChoosePacks {
                rule_index: 0,
                choices: vec![
                    "urn:ordin:acme:practice-packs:stack:go".to_owned(),
                    "urn:ordin:acme:practice-packs:stack:rust".to_owned()
                ]
            },
            Fix::LetAgentAsk { rule_index: 0 }
        ]
    );
    assert!(!report.passed);
}

#[tokio::test]
async fn error_apm_cannot_hold_part_of_the_draft() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let mut check = draft_check(
        clean_draft(),
        vec![workspace("clause-checker", &minimal_body())],
    );
    check.target_format = TargetFormat::Apm;
    check.loss_report = vec![ReportItem {
        what: "fixedPracticePacks".to_owned(),
        part_identifier: Some("urn:ordin:acme:workspaces:clause-checker".to_owned()),
        reason: "FMT_LOSS_NO_FIELD".to_owned(),
    }];
    let report = f.checker.check_agent_draft(&check).await.unwrap();
    let found = named(&report.findings, "format_loss");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Failure);
    assert_eq!(found[0].args["format"], "APM");
    assert_eq!(
        found[0].fixes,
        vec![
            Fix::LeaveItOut {
                what: "fixedPracticePacks".to_owned()
            },
            Fix::PublishToOrdinFormat
        ]
    );
}

#[tokio::test]
async fn error_publish_gate_not_met() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let agent = workspace(
        "clause-checker",
        &format!(
            "{}governanceMetadata:\n  publishGate: rehearsal\n",
            minimal_body()
        ),
    );
    let mut check = draft_check(clean_draft(), vec![agent]);
    check.rehearsal = Some(vec![
        ExpectationOutcome {
            rule: ExpectationRule::ApprovalAsked,
            met: true,
            evidence: "1 approvals asked".to_owned(),
        },
        ExpectationOutcome {
            rule: ExpectationRule::ToolNotUsed {
                tool: "Bash".to_owned(),
            },
            met: false,
            evidence: "tool Bash used once".to_owned(),
        },
    ]);
    let report = f.checker.check_agent_draft(&check).await.unwrap();
    let found = named(&report.findings, "publish_gate_not_met");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(found[0].severity, Severity::Failure);
    assert!(found[0].detail.contains("tool_not_used"));
    assert!(!report.publish_gate_met);
    assert!(!report.passed);
    // Not rehearsed at all.
    check.rehearsal = None;
    let report = f.checker.check_agent_draft(&check).await.unwrap();
    assert_eq!(
        named(&report.findings, "publish_gate_not_met")[0].args["reason"],
        "not rehearsed"
    );
}

#[tokio::test]
async fn edge_bare_command_name_is_only_a_warning() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let command = part(
        "models/@acme/commands/apply.md",
        &markdown(
            "command",
            "urn:ordin:acme:commands:apply",
            "Apply",
            "Apply the plan.\n",
        ),
    );
    let review = skill(
        "acme",
        "review",
        "1.0.0",
        "Read the plan, then run /apply next.\n",
    );
    let agent = workspace(
        "cspa-431",
        &format!(
            "{}commands:\n  - urn:ordin:acme:commands:apply\nskills:\n  - urn:ordin:acme:skills:review\n",
            minimal_body()
        ),
    );
    let report = f
        .checker
        .check_agent_draft(&draft_check(clean_draft(), vec![agent, command, review]))
        .await
        .unwrap();
    let found = named(&report.findings, "bare_command_name");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(
        found[0].fixes,
        vec![Fix::FixIt {
            action: FixAction::PointToReplacement {
                field: "urn:ordin:acme:skills:review".to_owned(),
                from: "/apply".to_owned(),
                to: "/cspa-431:apply".to_owned()
            }
        }]
    );
    assert!(report.passed, "findings: {:?}", report.findings);
}

fn control_loop(slug: &str) -> ModelPart {
    part(
        &format!("models/@acme/rules/control-loop-{slug}.md"),
        &format!(
            "---\napiVersion: ai.enterprise/v1\nkind: control-loop\nmetadata:\n  id: urn:ordin:acme:rules:control-loops:{slug}\n  name: {slug}\n  version: 1.0.0\n  owner: legal-ops\nstrategy: observe-then-act\ntrigger: every-task\nrequires: [evidence]\n---\nObserve first.\n"
        ),
    )
}

#[tokio::test]
async fn edge_a_change_rebuilds_every_agent_that_uses_the_part() {
    let mut home = base();
    home.push(control_loop("loop-a"));
    home.push(control_loop("loop-b"));
    home.push(rule("shared", "Cite the clause.\n"));
    home.push(workspace(
        "agent-a",
        &format!("{}  - urn:ordin:acme:rules:shared\n", minimal_body()),
    ));
    home.push(workspace(
        "agent-b",
        &format!(
            "{}  - urn:ordin:acme:rules:shared\ncontrolLoop: urn:ordin:acme:rules:control-loops:loop-a\n",
            minimal_body()
        ),
    ));
    let f = Fixture::with(source_of(home, Vec::new())).await;
    let changed = part(
        "models/@acme/rules/shared.md",
        "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:shared\n  name: shared\n  version: 1.1.0\n  owner: legal-ops\ncontrolLoop: urn:ordin:acme:rules:control-loops:loop-b\n---\nCite the clause.\n",
    );
    let report = f
        .checker
        .check_part_draft(
            &repository_check(vec![staged(changed)]),
            PartDraftId::new(),
            &BTreeMap::new(),
        )
        .await
        .unwrap();
    let failed = named(&report.findings, "agent_rebuild_failed");
    assert_eq!(failed.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(failed[0].args["agent"], "agent-b");
    assert_eq!(failed[0].args["reason"], "RES_LOOP_MULTIPLE");
    assert!(!report.passed);
    assert!(has(&report.findings, "repository_checked"));
}

pub(super) fn withdrawal_entry(
    id: &str,
    replacement: Option<&str>,
    reason: &str,
) -> WithdrawnListEntry {
    let at = Timestamp::from_unix_millis(1_768_000_000_000).unwrap();
    WithdrawnListEntry {
        id: WithdrawnListEntryId::new(),
        agent_repository_id: ordin_types::AgentRepositoryId(uuid::uuid!(
            "01900000-0000-7000-8000-0000000000a1"
        )),
        part_kind: PartKind::Skill,
        part_identifier: id.to_owned(),
        part_version: None,
        part_content_digest: None,
        reason: reason.to_owned(),
        withdrawn_at: at,
        replacement_identifier: replacement.map(str::to_owned),
        replacement_version: None,
        removed_by_hand_seen_at: None,
        read_at: at,
    }
}

#[tokio::test]
async fn edge_a_withdrawal_is_never_blocked_by_agents_that_would_stop() {
    let mut home = base();
    home.push(skill("acme", "old-review", "1.0.0", "Old checklist.\n"));
    // The replacement brings a rule that is not in the repository: agent C cannot rebuild with it.
    home.push(part(
        "models/@acme/skills/new-review/SKILL.md",
        "---\napiVersion: ai.enterprise/v1\nkind: skill\nmetadata:\n  id: urn:ordin:acme:skills:new-review\n  name: new-review\n  version: 1.0.0\n  owner: legal-ops\nrules:\n  - urn:ordin:acme:rules:missing\n---\nNew checklist.\n",
    ));
    home.push(workspace(
        "agent-c",
        &format!(
            "{}skills:\n  - urn:ordin:acme:skills:old-review\n",
            minimal_body()
        ),
    ));
    let f = Fixture::with(source_of(home, Vec::new())).await;
    let entry = withdrawal_entry(
        "urn:ordin:acme:skills:old-review",
        Some("urn:ordin:acme:skills:new-review"),
        "The old checklist is no longer valid.",
    );
    let check = WithdrawalCheck {
        repository: repository_check(Vec::new()),
        entry: entry.clone(),
        existing_entries: Vec::new(),
        proposed_list: vec![entry],
    };
    let report = f
        .checker
        .check_withdrawal(&check, PartDraftId::new())
        .await
        .unwrap();
    assert!(report.passed, "findings: {:?}", report.findings);
    assert_eq!(
        report.agents_that_stop,
        vec!["urn:ordin:acme:workspaces:agent-c".to_owned()]
    );
    let stops = named(&report.findings, "agent_stops_on_withdrawal");
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0].severity, Severity::Warning);
    assert_eq!(stops[0].args["agent"], "agent-c");
}

#[tokio::test]
async fn error_a_withdrawal_that_edits_an_earlier_entry() {
    let mut home = base();
    home.push(skill("acme", "old-review", "1.0.0", "Old checklist.\n"));
    home.push(skill("acme", "older-review", "1.0.0", "Older checklist.\n"));
    let f = Fixture::with(source_of(home, Vec::new())).await;
    let earlier = withdrawal_entry("urn:ordin:acme:skills:older-review", None, "Retired.");
    let mut edited = earlier.clone();
    edited.reason = "Retired for good.".to_owned();
    let entry = withdrawal_entry("urn:ordin:acme:skills:old-review", None, "Replaced.");
    let check = WithdrawalCheck {
        repository: repository_check(Vec::new()),
        entry: entry.clone(),
        existing_entries: vec![earlier],
        proposed_list: vec![edited, entry],
    };
    let report = f
        .checker
        .check_withdrawal(&check, PartDraftId::new())
        .await
        .unwrap();
    let found = named(&report.findings, "withdrawn_list_not_append");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(found[0].severity, Severity::Failure);
    assert!(!report.passed);
}

#[tokio::test]
async fn error_retiring_a_status_term() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let check = VocabularyCheck {
        repository: repository_check(Vec::new()),
        axis: ordin_agent_model::VocabularyAxis::ArtefactStatus,
        added: Vec::new(),
        retired: vec!["deprecated".to_owned()],
    };
    let report = f
        .checker
        .check_vocabulary(&check, PartDraftId::new())
        .await
        .unwrap();
    let found = named(&report.findings, "vocabulary_status_term_fixed");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(found[0].severity, Severity::Failure);
    assert!(!report.passed);
}

#[tokio::test]
async fn happy_path_check_it_on_an_installed_agent() {
    let mut home = base();
    home.push(workspace("clause-checker", &minimal_body()));
    let source = source_of(home, Vec::new());
    let f = Fixture::with(source.clone()).await;
    let shared: Arc<dyn PartSource> = source;
    let request = resolve_request(
        &scope(),
        "urn:ordin:acme:workspaces:clause-checker",
        Vec::new(),
        Vec::new(),
        false,
        &shared,
    );
    let lock = compile_instructions(shared.as_ref(), &request)
        .await
        .unwrap()
        .lock;
    let report = f
        .checker
        .check_installed(AgentInstanceId::new(), &lock, &scope())
        .await
        .unwrap();
    assert!(report.passed, "findings: {:?}", report.findings);
    assert_eq!(report.version_checked, lock.plan_hash);
    assert!(report.findings.is_empty());
}

#[tokio::test]
async fn edge_trust_lost_when_the_profile_changes() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let binding = "urn:ordin:acme:tool-bindings:files";
    f.trust
        .set(json!({ "tool_bindings": [{ "id": binding, "programs": ["files"] }] }));
    let instance = AgentInstanceId::new();
    let parts = [ExecutablePart {
        part_name: "files".to_owned(),
        kind: ExecutablePartKind::McpServer,
        declared_by: binding.to_owned(),
        bundled: false,
    }];
    let decided = f
        .checker
        .decide_executable_parts(instance, &parts)
        .await
        .unwrap();
    assert_eq!(decided[0].decision, ExecutablePartTrust::Trusted);
    f.trust.set(json!({}));
    let changes = f.checker.reevaluate_trust().await.unwrap();
    assert_eq!(changes.lost, vec![instance]);
    assert!(changes.gained.is_empty());
    let row: String = sqlx::query_scalar(
        "SELECT decision FROM executable_part_decisions WHERE agent_instance_id = ?1",
    )
    .bind(instance.to_string())
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(row, "needs_it_approval");
    // And back again.
    f.trust
        .set(json!({ "tool_bindings": [{ "id": binding, "programs": ["files"] }] }));
    let changes = f.checker.reevaluate_trust().await.unwrap();
    assert_eq!(changes.gained, vec![instance]);
    assert_eq!(
        f.journal_kinds().await,
        vec![
            "executable_parts_decided",
            "trust_reevaluated",
            "trust_reevaluated"
        ]
    );
}

#[tokio::test]
async fn edge_rehearsal_expectations() {
    let f = Fixture::unbound().await;
    let evidence = RunEvidence {
        tools_used: vec!["Read".to_owned()],
        ..RunEvidence::default()
    };
    let outcomes = f.checker.check_rehearsal(
        &[ExpectationRule::ToolNotUsed {
            tool: "Bash".to_owned(),
        }],
        &evidence,
    );
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].met);
}

#[tokio::test]
async fn error_a_check_before_the_source_is_bound() {
    let f = Fixture::unbound().await;
    let error = f
        .checker
        .check_repository(
            &repository_check(Vec::new()),
            CheckSubject::RepositoryRead {
                repository: ACME_LABEL.to_owned(),
                commit: "a".repeat(40),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, CheckError::SourceNotBound));
    assert_eq!(error.code(), "CHK_SOURCE_NOT_BOUND");
    assert!(!error.retryable());
    assert_eq!(f.runs().await, 0);
}

/// The plan of `root` resolved through `source`.
async fn plan_of(
    f: &Fixture,
    source: &Arc<dyn PartSource>,
    root: &str,
    allow_draft_root: bool,
) -> ordin_agent_model::ResolutionPlan {
    let request = resolve_request(
        &scope(),
        root,
        Vec::new(),
        Vec::new(),
        allow_draft_root,
        source,
    );
    f.resolver.resolve(&request).await.expect("a plan")
}

#[tokio::test]
async fn happy_path_a_trial_is_checked_through_the_draft_source() {
    // The bound source holds nothing of the draft; the draft source holds the draft agent.
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let mut draft_home = base();
    draft_home.push(agent_with_tools("clause-checker"));
    draft_home.push(jira());
    let draft_source: Arc<dyn PartSource> = source_of(draft_home, Vec::new());
    f.trust.set(json!({
        "tool_bindings": [{ "id": "urn:ordin:acme:tool-bindings:jira", "programs": ["jira-reader"] }]
    }));
    let plan = plan_of(
        &f,
        &draft_source,
        "urn:ordin:acme:workspaces:clause-checker",
        true,
    )
    .await;
    let checked = f
        .checker
        .check_for_install(&InstallCheck {
            plan,
            scope: scope(),
            agent_instance_id: None,
            source: Some(SourceOverride(Arc::clone(&draft_source))),
        })
        .await
        .unwrap();
    assert!(
        matches!(checked.verdict, InstallVerdict::Clean),
        "findings: {:?}",
        checked.report.findings
    );
    let kind: String = sqlx::query_scalar("SELECT subject_kind FROM check_runs")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(kind, "install_candidate");
    let decisions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM executable_part_decisions")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(decisions, 0);
}

#[tokio::test]
async fn edge_install_needs_it_approval() {
    let mut home = base();
    home.push(agent_with_tools("clause-checker"));
    home.push(jira());
    let source: Arc<dyn PartSource> = source_of(home, Vec::new());
    let f = Fixture::with(Arc::clone(&source)).await;
    f.trust.set(json!({ "tool_bindings": [] }));
    let plan = plan_of(
        &f,
        &source,
        "urn:ordin:acme:workspaces:clause-checker",
        false,
    )
    .await;
    let checked = f
        .checker
        .check_for_install(&InstallCheck {
            plan,
            scope: scope(),
            agent_instance_id: None,
            source: None,
        })
        .await
        .unwrap();
    match checked.verdict {
        InstallVerdict::NeedsItApproval { parts } => {
            let names: Vec<&str> = parts.iter().map(|p| p.part_name.as_str()).collect();
            assert_eq!(names, vec!["jira-reader"]);
        }
        other => panic!(
            "expected needs_it_approval, got {other:?}: {:?}",
            checked.report.findings
        ),
    }
}

/// A source whose only repository is the bundled catalogue, holding the agent with a program that
/// needs trust, and the scope that reads it.
fn bundled_source() -> (Arc<dyn PartSource>, SourceScope) {
    let mut home = base();
    home.push(agent_with_tools("clause-checker"));
    home.push(jira());
    let source: Arc<dyn PartSource> = Arc::new(ordin_agent_model::testkit::InMemorySource::new(
        &ordin_agent_model::testkit::ContractFixture {
            repositories: vec![(
                ordin_agent_model::RepositoryRef::Bundled,
                "bundled".to_owned(),
                home,
            )],
            withdrawn: Vec::new(),
            refused: Vec::new(),
        },
    ));
    let scope = SourceScope {
        home: ordin_agent_model::RepositoryRef::Bundled,
        profile: Vec::new(),
        include_bundled: true,
    };
    (source, scope)
}

async fn bundled_plan(
    f: &Fixture,
    source: &Arc<dyn PartSource>,
    scope: &SourceScope,
) -> ordin_agent_model::ResolutionPlan {
    let request = resolve_request(
        scope,
        "urn:ordin:acme:workspaces:clause-checker",
        Vec::new(),
        Vec::new(),
        false,
        source,
    );
    f.resolver.resolve(&request).await.expect("a plan")
}

// Scenario: Happy path — a bundled agent's program is trusted without the policy listing it (CHK-TR-1)
#[tokio::test]
async fn happy_path_a_bundled_agents_program_is_trusted_without_the_policy() {
    let (source, scope) = bundled_source();
    let f = Fixture::with(Arc::clone(&source)).await;
    f.trust.set(json!({ "tool_bindings": [] }));
    let plan = bundled_plan(&f, &source, &scope).await;
    let instance = AgentInstanceId::new();
    let checked = f
        .checker
        .check_for_install(&InstallCheck {
            plan,
            scope,
            agent_instance_id: Some(instance),
            source: None,
        })
        .await
        .unwrap();
    assert!(
        matches!(checked.verdict, InstallVerdict::Clean),
        "findings: {:?}",
        checked.report.findings
    );
    assert!(!checked
        .report
        .findings
        .iter()
        .any(|f| f.code == "trust_needs_it_approval"));
    let parts = f.checker.executable_parts(instance).await.unwrap();
    assert!(
        !parts.is_empty() && parts.iter().all(|p| p.bundled),
        "{parts:?}"
    );
    let decided = f
        .checker
        .decide_executable_parts(instance, &parts)
        .await
        .unwrap();
    assert!(decided
        .iter()
        .all(|d| d.decision == ExecutablePartTrust::Trusted));
}

// Scenario: Edge case — a new profile does not take the trust back (CHK-TR-1)
#[tokio::test]
async fn edge_a_new_profile_does_not_take_a_bundled_agents_trust_back() {
    let (source, scope) = bundled_source();
    let f = Fixture::with(Arc::clone(&source)).await;
    let binding = "urn:ordin:acme:tool-bindings:jira";
    f.trust
        .set(json!({ "tool_bindings": [{ "id": binding, "programs": ["jira-reader"] }] }));
    let plan = bundled_plan(&f, &source, &scope).await;
    let instance = AgentInstanceId::new();
    f.checker
        .check_for_install(&InstallCheck {
            plan,
            scope,
            agent_instance_id: Some(instance),
            source: None,
        })
        .await
        .unwrap();
    let parts = f.checker.executable_parts(instance).await.unwrap();
    f.checker
        .decide_executable_parts(instance, &parts)
        .await
        .unwrap();
    f.trust.set(json!({}));
    let changes = f.checker.reevaluate_trust().await.unwrap();
    assert!(changes.lost.is_empty(), "{changes:?}");
}

// Scenario: Security — the same program in an added repository is not exempt (CHK-TR-1)
#[tokio::test]
async fn security_the_same_program_in_an_added_repository_is_not_exempt() {
    let mut home = base();
    home.push(agent_with_tools("clause-checker"));
    home.push(jira());
    let source: Arc<dyn PartSource> = source_of(home, Vec::new());
    let f = Fixture::with(Arc::clone(&source)).await;
    f.trust.set(json!({ "tool_bindings": [] }));
    let plan = plan_of(
        &f,
        &source,
        "urn:ordin:acme:workspaces:clause-checker",
        false,
    )
    .await;
    let checked = f
        .checker
        .check_for_install(&InstallCheck {
            plan,
            scope: scope(),
            agent_instance_id: Some(AgentInstanceId::new()),
            source: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        checked.verdict,
        InstallVerdict::NeedsItApproval { .. }
    ));
    assert!(checked
        .report
        .findings
        .iter()
        .any(|f| f.code == "trust_needs_it_approval"));
}

// Scenario: Edge case — a part stored before the amendment (CHK-TR-1)
#[test]
fn edge_an_executable_part_stored_without_bundled_reads_as_not_bundled() {
    let part: ExecutablePart = serde_json::from_value(json!({
        "part_name": "files",
        "kind": "mcp_server",
        "declared_by": "urn:ordin:acme:tool-bindings:files"
    }))
    .unwrap();
    assert!(!part.bundled);
    assert!(!serde_json::to_string(&part).unwrap().contains("bundled"));
}

#[tokio::test]
async fn happy_path_the_complete_executable_part_list_of_an_instance() {
    let mut home = base();
    home.push(agent_with_tools("clause-checker"));
    home.push(tool_binding(
        "jira",
        &format!("{}{}", mcp("files"), mcp("jira-reader")),
    ));
    let source: Arc<dyn PartSource> = source_of(home, Vec::new());
    let f = Fixture::with(Arc::clone(&source)).await;
    f.trust.set(json!({
        "tool_bindings": [{ "id": "urn:ordin:acme:tool-bindings:jira", "programs": ["files"] }]
    }));
    let plan = plan_of(
        &f,
        &source,
        "urn:ordin:acme:workspaces:clause-checker",
        false,
    )
    .await;
    let instance = AgentInstanceId::new();
    let checked = f
        .checker
        .check_for_install(&InstallCheck {
            plan,
            scope: scope(),
            agent_instance_id: Some(instance),
            source: None,
        })
        .await
        .unwrap();
    match &checked.verdict {
        InstallVerdict::NeedsItApproval { parts } => {
            let names: Vec<&str> = parts.iter().map(|p| p.part_name.as_str()).collect();
            assert_eq!(names, vec!["jira-reader"]);
        }
        other => panic!("expected needs_it_approval, got {other:?}"),
    }
    let mut all: Vec<String> = f
        .checker
        .executable_parts(instance)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.part_name)
        .collect();
    all.sort();
    assert_eq!(all, vec!["files", "jira-reader"]);
}

#[tokio::test]
async fn error_executable_parts_of_an_instance_never_checked() {
    let f = Fixture::unbound().await;
    let instance = AgentInstanceId::new();
    let error = f.checker.executable_parts(instance).await.unwrap_err();
    assert!(matches!(error, CheckError::InstanceUnknown { id } if id == instance));
    assert!(!error.retryable());
}

pub fn contract(slug: &str, sections: &str) -> ModelPart {
    part(
        &format!("models/@acme/output-contracts/{slug}.yaml"),
        &yaml(
            "output-contract",
            &format!("urn:ordin:acme:output-contracts:{slug}"),
            slug,
            "",
            &format!("sections:\n{sections}"),
        ),
    )
}

#[tokio::test]
async fn error_a_contract_section_without_a_name() {
    let f = Fixture::unbound().await;
    let bad = contract(
        "role-dossier",
        "  - name: purpose\n    required: true\n  - meaning: What it does.\n",
    );
    let error = f.checker.read_dossier("# Purpose\n", &bad).unwrap_err();
    assert!(matches!(
        error,
        CheckError::ContractInvalid {
            reason: "section_malformed"
        }
    ));
}

fn result(link: &str, local_only: bool, headings: Option<&[&str]>) -> HandoverResult {
    HandoverResult {
        link: link.to_owned(),
        label: "Review notes".to_owned(),
        local_only,
        readable_by_receiver: None,
        headings: headings.map(|h| h.iter().map(|s| (*s).to_owned()).collect()),
    }
}

#[tokio::test]
async fn error_a_handover_result_left_on_this_computer() {
    let f = Fixture::unbound().await;
    let report = f
        .checker
        .check_handover(&HandoverCheckInput {
            results: vec![result("file:///Users/asha/notes.md", true, None)],
            contract: None,
            receiver: ReceiverFacts {
                is_person: true,
                reachable: true,
            },
        })
        .await
        .unwrap();
    let readable: Vec<&HandoverFinding> = report
        .findings
        .iter()
        .filter(|f| f.code == "results_readable")
        .collect();
    assert_eq!(readable.len(), 1);
    assert!(!readable[0].passed);
    assert_eq!(
        readable[0].item.as_deref(),
        Some("file:///Users/asha/notes.md")
    );
    let reachable = report
        .findings
        .iter()
        .find(|f| f.code == "receiver_reachable")
        .unwrap();
    assert!(reachable.passed);
    assert!(!report.passed);
}

#[tokio::test]
async fn edge_handover_result_missing_a_contract_section() {
    let mut home = base();
    home.push(contract(
        "review",
        "  - name: summary\n    required: true\n  - name: risks\n    required: true\n  - name: appendix\n",
    ));
    let f = Fixture::with(source_of(home, Vec::new())).await;
    let report = f
        .checker
        .check_handover(&HandoverCheckInput {
            results: vec![result(
                "https://sharepoint.example/review.docx",
                false,
                Some(&["Summary", "Next steps"]),
            )],
            contract: Some(HandoverContract {
                scope: scope(),
                identifier: "urn:ordin:acme:output-contracts:review".to_owned(),
            }),
            receiver: ReceiverFacts {
                is_person: true,
                reachable: true,
            },
        })
        .await
        .unwrap();
    let sections = report
        .findings
        .iter()
        .find(|f| f.code == "contract_sections")
        .unwrap();
    assert!(!sections.passed);
    assert_eq!(sections.missing_sections, vec!["risks".to_owned()]);
    assert!(!report.passed);
}

#[tokio::test]
async fn happy_path_reading_a_role_dossier() {
    let f = Fixture::unbound().await;
    let dossier = contract(
        "role-dossier",
        "  - name: role-identity\n    required: true\n  - name: triggers-inputs\n    required: true\n  - name: boundaries\n    required: true\n  - name: escalation\n    required: true\n",
    );
    let write_up = "Intro text is dropped.\n# Role identity\n- Reviews supplier contracts\n## Triggers & inputs\n* A contract arrives\n";
    let reading = f.checker.read_dossier(write_up, &dossier).unwrap();
    let names: Vec<(&str, u32)> = reading
        .sections
        .iter()
        .map(|s| (s.name.as_str(), s.evidence_count))
        .collect();
    assert_eq!(names, vec![("role-identity", 1), ("triggers-inputs", 1)]);
    assert_eq!(reading.missing_sections, vec!["boundaries", "escalation"]);
    assert!(reading.unmatched_headings.is_empty());
}

/// A source that reads the tone rule as an APM package file with no `metadata.id`, as the Format
/// Plug-ins hand it over when the file carries none; resolution by kind and version only.
struct NoIdSource(Arc<dyn PartSource>);

#[async_trait::async_trait]
impl PartSource for NoIdSource {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, ordin_agent_model::PartSourceError> {
        let found = self.0.resolve(scope, kind, id, version).await?;
        Ok(match found {
            Resolution::Found(mut sourced) if version.is_some() && id == TONE_ID => {
                sourced.manifest.metadata.id = String::new();
                Resolution::Found(sourced)
            }
            other => other,
        })
    }
    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, ordin_agent_model::PartSourceError> {
        self.0.resolve_any_kind(scope, id).await
    }
    async fn files(
        &self,
        part: &SourcedPart,
    ) -> Result<ordin_agent_model::PartFiles, ordin_agent_model::PartSourceError> {
        self.0.files(part).await
    }
    async fn ingredients(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<SourcedPart>, ordin_agent_model::PartSourceError> {
        self.0.ingredients(scope).await
    }
    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, ordin_agent_model::PartSourceError> {
        self.0.withdrawn(scope).await
    }
    async fn repository_parts(
        &self,
        repository: &ordin_agent_model::RepositoryRef,
    ) -> Result<Vec<SourcedPart>, ordin_agent_model::PartSourceError> {
        self.0.repository_parts(repository).await
    }
    async fn vocabulary(
        &self,
        scope: &SourceScope,
    ) -> Result<ordin_agent_model::VocabularySet, ordin_agent_model::PartSourceError> {
        self.0.vocabulary(scope).await
    }
    async fn used_by(
        &self,
        part: &SourcedPart,
    ) -> Result<Vec<SourcedPart>, ordin_agent_model::PartSourceError> {
        self.0.used_by(part).await
    }
    async fn citing_parts(
        &self,
        repository: &ordin_agent_model::RepositoryRef,
        axis: ordin_agent_model::VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, ordin_agent_model::PartSourceError> {
        self.0.citing_parts(repository, axis, code).await
    }
}

#[tokio::test]
async fn error_package_finding_names_its_file() {
    let mut home = vocabularies();
    home.push(role());
    home.push(part(
        ".apm/instructions/tone.instructions.md",
        &markdown("rule", TONE_ID, "Tone", "Use short sentences.\n"),
    ));
    home.push(workspace("clause-checker", &minimal_body()));
    let inner: Arc<dyn PartSource> = source_of(home, Vec::new());
    let source: Arc<dyn PartSource> = Arc::new(NoIdSource(inner));
    let request = resolve_request(
        &scope(),
        "urn:ordin:acme:workspaces:clause-checker",
        Vec::new(),
        Vec::new(),
        false,
        &source,
    );
    let report = crate::check_parts(source.as_ref(), &request).await.unwrap();
    let stops: Vec<&PackageFinding> = report.findings.iter().filter(|f| f.stop).collect();
    assert_eq!(stops.len(), 1, "findings: {:?}", report.findings);
    assert_eq!(stops[0].file, ".apm/instructions/tone.instructions.md");
    assert_eq!(stops[0].entry, "metadata_required");
}

#[tokio::test]
async fn error_clone_unavailable() {
    let inner: Arc<dyn PartSource> = source_of(base(), Vec::new());
    let f = Fixture::with(Arc::new(FailingSource {
        inner,
        fails: Fails::RepositoryParts,
        error: ordin_agent_model::PartSourceError::Unavailable { retryable: true },
    }))
    .await;
    let error = f
        .checker
        .check_repository(
            &repository_check(Vec::new()),
            CheckSubject::RepositoryRead {
                repository: ACME_LABEL.to_owned(),
                commit: "a".repeat(40),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        CheckError::SourceUnavailable { retryable: true }
    ));
    assert_eq!(f.runs().await, 0);
}

/// A lifecycle with two stages; `channel` is the first stage's `slackChannel` block.
fn lifecycle(channel: &str) -> ModelPart {
    part(
        "models/@acme/journeys/contract-review.yaml",
        &yaml(
            "journey",
            "urn:ordin:acme:journeys:contract-review",
            "Contract review",
            "",
            &format!(
                "stages:\n  - id: design\n    name: Design\n    role: Designer\n    handoverPlace:\n      type: folder\n      target: shared-designs\n    answerWithinDays: 5\n{channel}  - id: review\n    name: Review\n    role: Reviewer\n    handoverPlace:\n      type: folder\n      target: shared-reviews\n"
            ),
        ),
    )
}

async fn lifecycle_findings(channel: &str) -> Vec<Finding> {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let report = f
        .checker
        .check_part_draft(
            &repository_check(vec![staged(lifecycle(channel))]),
            PartDraftId::new(),
            &BTreeMap::new(),
        )
        .await
        .unwrap();
    report
        .findings
        .into_iter()
        .filter(|f| f.code.starts_with("lifecycle_"))
        .collect()
}

#[tokio::test]
async fn a_valid_stage_channel_passes() {
    let findings = lifecycle_findings(
        "    slackChannel:\n      id: C04ABCDEF12\n      name: design-reviews\n",
    )
    .await;
    assert!(!has(&findings, "lifecycle_slack_channel_invalid"));
    assert!(findings.is_empty(), "findings: {findings:?}");
}

#[tokio::test]
async fn a_channel_name_typed_with_a_hash() {
    let findings = lifecycle_findings(
        "    slackChannel:\n      id: C04ABCDEF12\n      name: \"#design-reviews\"\n",
    )
    .await;
    let found = named(&findings, "lifecycle_slack_channel_invalid");
    assert_eq!(found.len(), 1, "findings: {findings:?}");
    assert_eq!(
        found[0].fixes,
        vec![Fix::ChangeItYourself {
            field: "slackChannel".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_stage_without_a_channel_is_not_checked_for_one() {
    let findings = lifecycle_findings("").await;
    // Step 8 before January 2026 finds nothing wrong with this lifecycle.
    assert!(findings.is_empty(), "findings: {findings:?}");
}

/// The stopping findings of an agent whose profile prohibits `entries` (CHK-TP-1).
async fn prohibited_stops(entries: &[&str]) -> Vec<PackageFinding> {
    let list: String = entries.iter().map(|e| format!("      - {e}\n")).collect();
    let body = format!(
        "{}toolPermissions:\n  workspaceOverrides:\n    prohibited:\n{list}",
        minimal_body()
    );
    let mut home = base();
    home.push(workspace("clause-checker", &body));
    let source: Arc<dyn PartSource> = source_of(home, Vec::new());
    let request = resolve_request(
        &scope(),
        "urn:ordin:acme:workspaces:clause-checker",
        Vec::new(),
        Vec::new(),
        false,
        &source,
    );
    let report = crate::check_parts(source.as_ref(), &request).await.unwrap();
    report.findings.into_iter().filter(|f| f.stop).collect()
}

// spec: ordin-desktop/components/Checker#CHK-TP-1
#[tokio::test]
async fn error_a_three_part_prohibited_entry() {
    let stops = prohibited_stops(&["git.commit", "github.pullrequest.create"]).await;
    // The part is reported once for each place it is checked from; every stop is this finding.
    assert!(!stops.is_empty(), "the part stops");
    for stop in &stops {
        assert_eq!(stop.entry, "urn:ordin:acme:workspaces:clause-checker");
        assert!(
            stop.message
                .contains("prohibited entry \"github.pullrequest.create\""),
            "the finding names the entry: {stops:?}"
        );
    }
}

// spec: ordin-desktop/components/Checker#CHK-TP-1
#[tokio::test]
async fn happy_commands_and_tool_names_pass_the_prohibited_check() {
    let stops = prohibited_stops(&["git.commit", "gh.pr", "secrets.read"]).await;
    assert!(
        stops.iter().all(|f| !f.message.contains("prohibited")),
        "findings: {stops:?}"
    );
}

// ---------------------------------------------------------------- Check it on an installed agent's offers

/// A language or style overlay part of the home repository.
fn overlay(kind: &str, axis: &str, tag: &str) -> ordin_agent_model::ModelPart {
    let key = if axis == "language" {
        "locale"
    } else {
        "style"
    };
    part(
        &format!("models/@acme/overlays/{axis}/{tag}/overlay.yaml"),
        &yaml(
            kind,
            &format!("urn:ordin:acme:overlays:{axis}:{tag}"),
            tag,
            "",
            &format!("{key}: {tag}\naddRules:\n  - {TONE_ID}\n"),
        ),
    )
}

/// An agent offering four languages and two styles, as the bundled document agent does, and the lock
/// it is installed with (the defaults chosen).
async fn agent_offering_choices(
    source: &Arc<ordin_agent_model::testkit::InMemorySource>,
) -> ordin_agent_model::LockFile {
    let shared: Arc<dyn PartSource> = source.clone();
    let request = resolve_request(
        &scope(),
        "urn:ordin:acme:workspaces:reader",
        Vec::new(),
        Vec::new(),
        false,
        &shared,
    );
    compile_instructions(shared.as_ref(), &request)
        .await
        .unwrap()
        .lock
}

fn offering_home() -> Vec<ordin_agent_model::ModelPart> {
    let mut home = base();
    for tag in ["en-GB", "en-US", "fr", "es"] {
        home.push(overlay("language-overlay", "language", tag));
    }
    for tag in ["plain-language", "concise"] {
        home.push(overlay("style-overlay", "style", tag));
    }
    let offers = "governanceMetadata:\n  languageOverlays:\n    available:\n      - urn:ordin:acme:overlays:language:en-GB\n      - urn:ordin:acme:overlays:language:en-US\n      - urn:ordin:acme:overlays:language:fr\n      - urn:ordin:acme:overlays:language:es\n    default: urn:ordin:acme:overlays:language:en-GB\n  styleOverlays:\n    available:\n      - urn:ordin:acme:overlays:style:plain-language\n      - urn:ordin:acme:overlays:style:concise\n    default: urn:ordin:acme:overlays:style:plain-language\n";
    home.push(workspace("reader", &format!("{}{offers}", minimal_body())));
    home
}

// Scenario: Happy path — an agent that offers languages it was not installed with (CHK-INS-1)
// spec: ordin-desktop/components/Checker#CHK-INS-1
#[tokio::test]
async fn happy_path_an_installed_agent_with_offers_it_did_not_choose() {
    let source = source_of(offering_home(), Vec::new());
    let f = Fixture::with(source.clone()).await;
    let lock = agent_offering_choices(&source).await;
    assert!(
        !lock
            .selections
            .overlays
            .iter()
            .any(|o| o.id.ends_with(":fr")),
        "{:?}",
        lock.selections
    );
    let report = f
        .checker
        .check_installed(AgentInstanceId::new(), &lock, &scope())
        .await
        .unwrap();
    assert!(
        report.findings.is_empty(),
        "findings: {:?}",
        report.findings
    );
    assert!(report.passed);
}

// Scenario: Error path — a chosen overlay missing from the lock (CHK-INS-1), reported once although
// the lock lists its profile twice (CHK-INS-2)
// spec: ordin-desktop/components/Checker#CHK-INS-1
// spec: ordin-desktop/components/Checker#CHK-INS-2
#[tokio::test]
async fn error_a_chosen_overlay_missing_from_the_lock_is_reported_once() {
    let source = source_of(offering_home(), Vec::new());
    let f = Fixture::with(source.clone()).await;
    let mut lock = agent_offering_choices(&source).await;
    let chosen_by = lock.selections.overlays[0].chosen_by;
    lock.selections
        .overlays
        .push(ordin_agent_model::SelectedPart {
            id: "urn:ordin:acme:overlays:style:concise".to_owned(),
            chosen_by,
        });
    if !lock.artefacts.iter().any(|a| a.id == lock.composition.id) {
        lock.artefacts.push(lock.composition.clone());
    }
    let report = f
        .checker
        .check_installed(AgentInstanceId::new(), &lock, &scope())
        .await
        .unwrap();
    let found = named(&report.findings, "reference_unresolved");
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    assert!(found[0].detail.contains("style:concise"), "{:?}", found[0]);
    assert_eq!(
        found[0].args.get("part").map(String::as_str),
        Some("reader")
    );
    assert!(!report.passed);
}

// Scenario: Edge case — every finding reads as a sentence (CHK-MSG-1). The ids are read from
// `finding.rs` itself, so a new id without words fails here.
// spec: ordin-desktop/components/Checker#CHK-MSG-1
#[test]
fn every_message_id_has_an_en_gb_string() {
    let root = workspace_root();
    let finding = std::fs::read_to_string(root.join("crates/ordin-checker/src/finding.rs"))
        .expect("finding.rs");
    let ids: Vec<&str> = finding
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub const "))
        .filter_map(|l| l.split('"').nth(1))
        .filter(|id| id.starts_with("check-"))
        .collect();
    assert_eq!(ids.len(), 25, "{ids:?}");
    let mut catalogue = String::new();
    let dir = root.join("crates/ordin-localisation/catalogues/en-GB");
    for entry in std::fs::read_dir(&dir).expect("the en-GB catalogue") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|e| e == "ftl") {
            catalogue.push_str(&std::fs::read_to_string(&path).expect("a catalogue file"));
            catalogue.push('\n');
        }
    }
    let missing: Vec<&&str> = ids
        .iter()
        .filter(|id| !catalogue.lines().any(|l| l.starts_with(&format!("{id} ="))))
        .collect();
    assert!(missing.is_empty(), "no en-GB string for {missing:?}");
}
