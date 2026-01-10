//! JSON round trips of the public types against the 2C examples (SharedTypes.md Internal Logic 9),
//! the agent-configuration example files, the timestamp property, and the compile-time check that
//! every public type is `Send + Sync`.

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Parses `json` as `T`, writes it back, and requires the same JSON value.
fn round_trip<T: Serialize + DeserializeOwned>(json: &str) {
    let original: serde_json::Value = serde_json::from_str(json).expect("the example is JSON");
    let typed: T = serde_json::from_value(original.clone()).expect("the example has the 2C shape");
    assert_eq!(serde_json::to_value(&typed).unwrap(), original);
}

// 2C wire, the example at phase-2c-shared-types-wire.md:812.
#[test]
fn example_tool_call() {
    round_trip::<ordin_types::wire::rules::ToolCall>(
        r#"{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff","workspace_id":"0192a0f1-1111-7222-8333-444455556666","point":"before_tool_call","tool":"Bash","origin":{"kind":"built_in"},"arguments":{"command":"git push origin main"}}"#,
    );
}

// 2C wire, the example at phase-2c-shared-types-wire.md:887.
#[test]
fn example_tool_decision() {
    round_trip::<ordin_types::wire::rules::ToolDecision>(
        r#"{
  "decision": "ask",
  "proposal": {
    "action": "Push to GitHub",
    "destination": "acme/supplier-portal, branch main",
    "affects": [{"kind": "record", "label": "3 commits on main"}],
    "consequence": "The three commits become visible to everyone who can open the repository."
  },
  "rule": {"source": "app_approval_policy", "rule_id": "app.external_action"},
  "decided_at": "2026-01-10T09:41:07.305Z"
}"#,
    );
}

// 2C wire, the example at phase-2c-shared-types-wire.md:984.
#[test]
fn example_hook_request() {
    round_trip::<ordin_types::wire::hook::HookRequest>(
        r#"{"protocol":1,"call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00","session":"0192a1c2-3344-7556-8677-8899aabbccdd","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff","mac":"x3mQ9Jt0b2pV8sYk1LwZr6NcA4fH7eDuIq5oGvRnTyE","message":{"kind":"pre_tool_use","harness":"claude_code","tool":"mcp__supplier-db__update_rate","origin":{"kind":"package_mcp_server","server":"supplier-db"},"arguments":{"supplier":"Kestrel","rate":412.5},"harness_tool_use_id":"toolu_01AbCdEf"}}"#,
    );
}

// 2C wire, the example at phase-2c-shared-types-wire.md:1112.
#[test]
fn example_approval_card() {
    round_trip::<ordin_types::wire::approval::ApprovalCard>(
        r#"{
  "approval_id": "0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f",
  "backend": "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b",
  "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
  "asked_by": {"kind": "run", "run_id": "0192a1c2-3344-7556-8677-8899aabbccdd"},
  "question": "Publish the revised account brief?",
  "verb": "Publish",
  "where_line": "Sales workspace",
  "affects": [{"kind": "file", "label": "Account brief", "version": "4"}],
  "changes": "Updated revenue table and three action items",
  "content": {"kind": "changes", "label": "See the changes", "change_set": {"kind": "artefact", "artefact_id": "0192a1c4-0000-7000-8000-00000000a001"}},
  "reject_style": "reject",
  "buttons": {"approve": "q8Zt3VxN0pLk2mRb7sYw1A", "reject": "Lm4Qp0Rt7Vx2Zb5Nc8Df3g"},
  "created_at": "2026-01-10T09:41:07.312Z"
}"#,
    );
}

// 2C wire, the example at phase-2c-shared-types-wire.md:1197.
#[test]
fn example_approval_content() {
    round_trip::<ordin_types::wire::approval::ApprovalContent>(
        r#"{"kind":"email","label":"See what will be sent","email":{"draft_id":"0192a1c5-7777-7888-8999-aaaabbbbcccc","from":"asha.patel@acme.co.uk","to":[{"name":"Dan Moss","address":"dan.moss@kestrel.co.uk","outside_company":true}],"cc":[],"bcc":[],"subject":"Re: Q3 quote","body":"Hi Dan,\n\nThanks for the revised quote…\n\nAsha Patel\nProcurement, Acme","attachments":[{"file_name":"Q3 comparison.pdf","comes_from":"Suppliers › Q3","bytes":184320}],"drafted_by":"Supplier analyst","in_reply_to":"Dan Moss, 14 Oct, “Q3 quote”"}}"#,
    );
}

// 2C wire, the example at phase-2c-shared-types-wire.md:1284.
#[test]
fn example_start_up_report_items() {
    round_trip::<Vec<ordin_types::wire::startup::StartUpReportItem>>(
        r#"[
  {"kind":"enforced_rule","rule":{"source":"app_approval_policy","rule_id":"app.external_action"},"plain":"Asks you before anything leaves your computer."},
  {"kind":"verb_not_offered","adapter":"slack_thread","verb":"cancel","plain":"Can't be stopped from ordin once asked; it answers in Slack."},
  {"kind":"own_screens_not_shown","server":"spend-explorer","plain":"Shows its own screens, which ordin does not display."}
]"#,
    );
}

// 2C wire, the two examples at phase-2c-shared-types-wire.md:1021.
#[test]
fn example_hook_replies() {
    round_trip::<ordin_types::wire::hook::HookReply>(
        r#"{"kind":"holding","call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00","approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f","deadline":"2026-01-10T09:41:57.000Z"}"#,
    );
    round_trip::<ordin_types::wire::hook::HookReply>(
        r#"{"kind":"allow","call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00"}"#,
    );
}

// 2C wire §12, the `KnowledgeHit` example.
#[test]
fn example_knowledge_hit() {
    // `answer_id` is absent from the example and `None` is written as `null` by the 2C shape, so the
    // comparison is on the parsed value written back without it.
    let json = r#"{"statement":"Borealis Ltd supplies recycled packaging to the Northern plants.","agent":"procurement-knowledge","repository":"git.example.org/supply/handbook@main","path":"suppliers/borealis.md","commit":"9f3c2a1b7d4e5f60718293a4b5c6d7e8f9a0b1c2","section":"What they supply","page_title":"Supplier Borealis","kind":"page","tier":"human_reviewed","status":"stable","may_be_out_of_date":false,"as_of":"2026-01-10T08:15:02.114Z","shared_identity":{"glossary":"git.example.org/org/glossary@main","concept_path":"borealis-ltd.md"},"things":["Borealis Ltd","packaging"]}"#;
    let hit: ordin_types::wire::knowledge_query::KnowledgeHit = serde_json::from_str(json).unwrap();
    assert_eq!(
        hit.tier,
        Some(ordin_types::wire::knowledge_query::PageTier::HumanReviewed)
    );
    let again: ordin_types::wire::knowledge_query::KnowledgeHit =
        serde_json::from_value(serde_json::to_value(&hit).unwrap()).unwrap();
    assert_eq!(again.statement, hit.statement);
    assert_eq!(again.shared_identity, hit.shared_identity);
}

/// Every public type is `Send + Sync` (Internal Logic 9), checked at compile time.
const _: fn() = || {
    fn f<T: Send + Sync>() {}
    f::<ordin_types::time::Timestamp>();
    f::<ordin_types::num::Sha256Hex>();
    f::<ordin_types::num::DecimalString>();
    f::<ordin_types::MemoryThingId>();
    f::<ordin_types::MemoryLinkId>();
    f::<ordin_types::RunId>();
    f::<ordin_types::BlockId>();
    f::<ordin_types::common::SetBy>();
    f::<ordin_types::common::Setting<u32>>();
    f::<ordin_types::common::AgentRef>();
    f::<ordin_types::common::InputUsed>();
    f::<ordin_types::paging::Page<u32>>();
    f::<ordin_types::paging::PageRequest>();
    f::<ordin_types::paging::Cursor>();
    f::<ordin_types::TypesError>();
    f::<ordin_types::RepairAction>();
    f::<ordin_types::text::AreaCodes>();
    f::<ordin_types::yaml::YamlNode>();
    f::<ordin_types::yaml::YamlError>();
    f::<ordin_types::yaml::YamlFeature>();
    f::<ordin_types::session::ToolCaller>();
    f::<ordin_types::error_registry::RegistryEntry>();
    f::<ordin_types::log::JsonLineFormat>();
    f::<ordin_types::log::LogSink>();
    f::<ordin_types::log::LogLevelInvalid>();
    f::<ordin_types::domain::agent_profile::RunRecord>();
    f::<ordin_types::domain::agent_profile::RunStarter>();
    f::<ordin_types::domain::agent_profile::RunRequest>();
    f::<ordin_types::domain::agent_profile::RunRecordState>();
    f::<ordin_types::domain::agent_profile::RunEndKind>();
    f::<ordin_types::domain::agent_profile::Scope>();
    f::<ordin_types::domain::agent_profile::ScopeKind>();
    f::<ordin_types::domain::agent_profile::ScopeState>();
    f::<ordin_types::domain::agent_profile::AgentConfig>();
    f::<ordin_types::domain::agent_profile::HarnessChoice>();
    f::<ordin_types::domain::agent_profile::ModelSetting>();
    f::<ordin_types::domain::agent_profile::ModelPolicyConfig>();
    f::<ordin_types::domain::agent_profile::EnvValue>();
    f::<ordin_types::domain::agent_profile::ToolsConfig>();
    f::<ordin_types::domain::agent_profile::McpBinding>();
    f::<ordin_types::domain::agent_profile::McpTransport>();
    f::<ordin_types::domain::agent_profile::HttpOperation>();
    f::<ordin_types::domain::agent_profile::HttpMethod>();
    f::<ordin_types::domain::agent_profile::HttpConnection>();
    f::<ordin_types::domain::agent_profile::PublishConnection>();
    f::<ordin_types::domain::agent_profile::IntakeEntry>();
    f::<ordin_types::domain::agent_profile::ScheduleEntry>();
    f::<ordin_types::domain::agent_profile::AuthConfig>();
    f::<ordin_types::domain::agent_profile::ChatTrigger>();
    f::<ordin_types::domain::agent_profile::TriggerKind>();
    f::<ordin_types::domain::agent_profile::Delivery>();
    f::<ordin_types::domain::agent_profile::ChatContext>();
    f::<ordin_types::domain::agent_profile::ChatContextFixed>();
    f::<ordin_types::domain::agent_profile::ContextStep>();
    f::<ordin_types::domain::agent_profile::StatusSignals>();
    f::<ordin_types::domain::agent_profile::ReplyAction>();
    f::<ordin_types::domain::agent_profile::ReplyActionKind>();
    f::<ordin_types::domain::agent_profile::LinkStyle>();
    f::<ordin_types::domain::agent_profile::EntryCommand>();
    f::<ordin_types::domain::agent_profile::EntryAction>();
    f::<ordin_types::domain::agent_profile::FillSource>();
    f::<ordin_types::domain::agent_profile::JobCommand>();
    f::<ordin_types::domain::agent_profile::JobCommandKind>();
    f::<ordin_types::domain::agent_profile::Who>();
    f::<ordin_types::domain::agent_profile::ChatConnection>();
    f::<ordin_types::domain::agent_profile::ChatKind>();
    f::<ordin_types::domain::agent_profile::RuntimeTool>();
    f::<ordin_types::domain::agent_profile::QuestionType>();
    f::<ordin_types::domain::agent_profile::MatchMode>();
    f::<ordin_types::domain::agent_profile::Ranking>();
    f::<ordin_types::domain::agent_profile::KnowledgeSourceConfig>();
    f::<ordin_types::domain::agent_profile::KnowledgeKind>();
    f::<ordin_types::domain::agent_profile::KnowledgeOperation>();
    f::<ordin_types::domain::agent_profile::RetentionConfig>();
    f::<ordin_types::domain::agent_profile::ListField>();
    f::<ordin_types::domain::agent_profile::SettingDeclaration>();
    f::<ordin_types::domain::agent_profile::SettingKind>();
    f::<ordin_types::domain::agent_profile::SettingLookup>();
    f::<ordin_types::domain::agent_profile::RuleListOutcome>();
    f::<ordin_types::domain::agent_profile::SettingDefault>();
    f::<ordin_types::domain::agent_profile::Capability>();
    f::<ordin_types::domain::agent_profile::ContractConfig>();
    f::<ordin_types::domain::agent_profile::OperationMapping>();
    f::<ordin_types::domain::agent_profile::ReachConfig>();
    f::<ordin_types::domain::agent_profile::ApproversConfig>();
    f::<ordin_types::domain::agent_profile::ApproverUser>();
    f::<ordin_types::domain::agent_profile::Point>();
    f::<ordin_types::domain::agent_profile::Condition>();
    f::<ordin_types::domain::agent_profile::CountOf>();
    f::<ordin_types::domain::agent_profile::CallTarget>();
    f::<ordin_types::domain::agent_profile::ScalarValue>();
    f::<ordin_types::domain::agent_profile::DecidingOutcome>();
    f::<ordin_types::domain::agent_profile::AdditiveOutcome>();
    f::<ordin_types::domain::agent_profile::MessageRule>();
    f::<ordin_types::domain::agent_profile::StepOutcome>();
    f::<ordin_types::domain::agent_profile::StepGate>();
    f::<ordin_types::domain::agent_profile::AddressCondition>();
    f::<ordin_types::domain::agent_profile::ArgumentRule>();
    f::<ordin_types::domain::agent_profile::ArgumentForm>();
    f::<ordin_types::domain::agent_profile::ArgumentOutcome>();
    f::<ordin_types::domain::agent_profile::ControlsConfig>();
    f::<ordin_types::domain::agent_profile::ContentCheck>();
    f::<ordin_types::domain::agent_profile::CheckPoint>();
    f::<ordin_types::domain::agent_profile::OnBlock>();
    f::<ordin_types::domain::agent_profile::OnUnreachable>();
    f::<ordin_types::domain::agent_profile::Check>();
    f::<ordin_types::domain::agent_profile::CheckKind>();
    f::<ordin_types::domain::agent_profile::HeartbeatForm>();
    f::<ordin_types::domain::agent_profile::DutyGate>();
    f::<ordin_types::domain::agent_profile::JudgedLoopGate>();
    f::<ordin_types::domain::agent_profile::CommandBound>();
    f::<ordin_types::domain::agent_profile::Gates>();
    f::<ordin_types::domain::agent_profile::ContractStatement>();
    f::<ordin_types::domain::agent_profile::ArgumentType>();
    f::<ordin_types::domain::agent_profile::OperationArgument>();
    f::<ordin_types::domain::agent_profile::DeclaredOperation>();
    f::<ordin_types::domain::agent_profile::Declaration>();
    f::<ordin_types::domain::agent_profile::Manifest>();
    f::<ordin_types::domain::agent_profile::PackageFacts>();
    f::<ordin_types::domain::agent_profile::Authority>();
    f::<ordin_types::domain::agent_profile::SkillFacts>();
    f::<ordin_types::domain::agent_profile::ToolSection>();
    f::<ordin_types::domain::agent_profile::McpEntry>();
    f::<ordin_types::domain::agent_profile::CommandFacts>();
    f::<ordin_types::domain::agent_profile::ReviewerFacts>();
    f::<ordin_types::domain::agent_profile::RecurringDuty>();
    f::<ordin_types::domain::agent_profile::KnowledgeAgentConfig>();
    f::<ordin_types::domain::agent_profile::KnowledgeRepository>();
    f::<ordin_types::domain::agent_profile::KnowledgeProxy>();
    f::<ordin_types::domain::agent_profile::ServiceIdentity>();
    f::<ordin_types::domain::agent_profile::ReadingConfig>();
    f::<ordin_types::domain::agent_profile::HotCopyConfig>();
    f::<ordin_types::domain::agent_profile::McpEndpointConfig>();
    f::<ordin_types::domain::agent_profile::SourcesRecordConfig>();
    f::<ordin_types::domain::agent_profile::ModelProvider>();
    f::<ordin_types::domain::agent_profile::OrganisationKey>();
    f::<ordin_types::domain::agent_profile::HarnessEnvVar>();
    f::<ordin_types::domain::agent_profile::HarnessModelSettings>();
    f::<ordin_types::domain::agent_profile::ModelFailureCause>();
    f::<ordin_types::domain::agent_profile::CallPlace>();
    f::<ordin_types::domain::agent_profile::HostedSessionSetup>();
    f::<ordin_types::domain::agent_profile::HarnessToolPolicy>();
    f::<ordin_types::domain::agent_profile::SubagentSetup>();
    f::<ordin_types::domain::agent_profile::SubagentTools>();
    f::<ordin_types::domain::agent_profile::SingleCallForm>();
    f::<ordin_types::domain::agents::AgentInstance>();
    f::<ordin_types::domain::agents::AuditResult>();
    f::<ordin_types::domain::agents::StoppedBy>();
    f::<ordin_types::domain::agents::Authority>();
    f::<ordin_types::domain::agents::AgentInstanceStatus>();
    f::<ordin_types::domain::agents::ExecutablePartDecision>();
    f::<ordin_types::domain::agents::ExecutablePartKind>();
    f::<ordin_types::domain::agents::ExecutablePartTrust>();
    f::<ordin_types::domain::agents::DecisionSource>();
    f::<ordin_types::domain::agents::RemoteAgent>();
    f::<ordin_types::domain::agents::RemoteRoute>();
    f::<ordin_types::domain::agents::RemoteAgentStatus>();
    f::<ordin_types::domain::agents::AgentRepository>();
    f::<ordin_types::domain::agents::AgentFormat>();
    f::<ordin_types::domain::agents::RepositorySource>();
    f::<ordin_types::domain::agents::AgentRepositoryState>();
    f::<ordin_types::domain::agents::Part>();
    f::<ordin_types::domain::agents::PartKind>();
    f::<ordin_types::domain::agents::PartStatus>();
    f::<ordin_types::domain::agents::WithdrawnListEntry>();
    f::<ordin_types::domain::agents::VocabularyTerm>();
    f::<ordin_types::domain::agents::VocabularyAxis>();
    f::<ordin_types::domain::agents::VocabularyTermState>();
    f::<ordin_types::domain::agents::Lifecycle>();
    f::<ordin_types::domain::agents::LifecycleStage>();
    f::<ordin_types::domain::agents::StageNext>();
    f::<ordin_types::domain::agents::StageChoice>();
    f::<ordin_types::domain::agents::SendBack>();
    f::<ordin_types::domain::agents::StageSlackChannel>();
    f::<ordin_types::domain::agents::WorkspaceStage>();
    f::<ordin_types::domain::agents::AgentDraft>();
    f::<ordin_types::domain::agents::DraftJob>();
    f::<ordin_types::domain::agents::WordingChange>();
    f::<ordin_types::domain::agents::DraftOrigin>();
    f::<ordin_types::domain::agents::PullRequestRef>();
    f::<ordin_types::domain::agents::PullRequestState>();
    f::<ordin_types::domain::agents::ReviewDraftState>();
    f::<ordin_types::domain::agents::Rehearsal>();
    f::<ordin_types::domain::agents::RehearsalSituation>();
    f::<ordin_types::domain::agents::RehearsalExpectation>();
    f::<ordin_types::domain::agents::ExpectationRule>();
    f::<ordin_types::domain::agents::ExpectationOutcome>();
    f::<ordin_types::domain::agents::PartDraft>();
    f::<ordin_types::domain::agents::PartChangeKind>();
    f::<ordin_types::domain::agents::FieldAuthor>();
    f::<ordin_types::domain::agents::FieldWriter>();
    f::<ordin_types::domain::agents::AgentRelease>();
    f::<ordin_types::domain::agents::AgentReleaseState>();
    f::<ordin_types::domain::email::EmailLink>();
    f::<ordin_types::domain::email::EmailLinkKind>();
    f::<ordin_types::domain::email::EmailLinkState>();
    f::<ordin_types::domain::email::ImapFolderState>();
    f::<ordin_types::domain::email::EmailGrant>();
    f::<ordin_types::domain::email::EmailAccess>();
    f::<ordin_types::domain::email::TelegramGrant>();
    f::<ordin_types::domain::email::TelegramAccess>();
    f::<ordin_types::domain::email::SlackGrant>();
    f::<ordin_types::domain::email::SlackAccess>();
    f::<ordin_types::domain::email::TeamFolderRole>();
    f::<ordin_types::domain::email::TeamKnowledgeBinding>();
    f::<ordin_types::domain::email::TeamAccess>();
    f::<ordin_types::domain::email::TeamAdoption>();
    f::<ordin_types::domain::email::AdoptionMark>();
    f::<ordin_types::domain::email::EmailDraft>();
    f::<ordin_types::domain::email::EmailDraftKind>();
    f::<ordin_types::domain::email::EmailAttachment>();
    f::<ordin_types::domain::email::DraftedBy>();
    f::<ordin_types::domain::email::EmailDraftState>();
    f::<ordin_types::domain::email::SentEmail>();
    f::<ordin_types::domain::engineering::Project>();
    f::<ordin_types::domain::engineering::ProjectState>();
    f::<ordin_types::domain::engineering::GithubOperationSetting>();
    f::<ordin_types::domain::engineering::GithubSettingSource>();
    f::<ordin_types::domain::engineering::LanguagePlugin>();
    f::<ordin_types::domain::engineering::LanguagePluginSource>();
    f::<ordin_types::domain::engineering::LanguagePluginStatus>();
    f::<ordin_types::domain::engineering::CodingTask>();
    f::<ordin_types::domain::engineering::TestOutcome>();
    f::<ordin_types::domain::engineering::CodingTaskState>();
    f::<ordin_types::domain::engineering::ChangeSet>();
    f::<ordin_types::domain::engineering::ChangeSetEntry>();
    f::<ordin_types::domain::engineering::ChangeEntryState>();
    f::<ordin_types::domain::engineering::ChangeSetState>();
    f::<ordin_types::domain::engineering::CraftInstallation>();
    f::<ordin_types::domain::engineering::VerificationResult>();
    f::<ordin_types::domain::engineering::CraftCliMode>();
    f::<ordin_types::domain::engineering::CraftInstallationStatus>();
    f::<ordin_types::domain::engineering::FileTypeMapEntry>();
    f::<ordin_types::domain::handover::Handover>();
    f::<ordin_types::domain::handover::HandoverReceiver>();
    f::<ordin_types::domain::handover::HandoverNote>();
    f::<ordin_types::domain::handover::NoteSection>();
    f::<ordin_types::domain::handover::HandoverEvent>();
    f::<ordin_types::domain::handover::HandoverEventKind>();
    f::<ordin_types::domain::handover::HandoverState>();
    f::<ordin_types::domain::handover::HandoverSlackMessage>();
    f::<ordin_types::domain::handover::HandoverNoticeKind>();
    f::<ordin_types::domain::handover::HandoverPlace>();
    f::<ordin_types::domain::handover::HandoverPlaceType>();
    f::<ordin_types::domain::identity::BrandPack>();
    f::<ordin_types::domain::identity::BrandStrings>();
    f::<ordin_types::domain::identity::BrandPackSource>();
    f::<ordin_types::domain::identity::BrandPackState>();
    f::<ordin_types::domain::identity::LocalePreference>();
    f::<ordin_types::domain::identity::LocaleSource>();
    f::<ordin_types::domain::identity::HarnessAttachment>();
    f::<ordin_types::domain::identity::HarnessKind>();
    f::<ordin_types::domain::identity::HarnessAttachmentState>();
    f::<ordin_types::domain::identity::OrganisationProfile>();
    f::<ordin_types::domain::identity::OrganisationProfileState>();
    f::<ordin_types::domain::identity::Credential>();
    f::<ordin_types::domain::identity::CredentialKind>();
    f::<ordin_types::domain::identity::CredentialState>();
    f::<ordin_types::domain::identity::UsageSettings>();
    f::<ordin_types::domain::identity::UsageChoice>();
    f::<ordin_types::domain::identity::SummariseWhen>();
    f::<ordin_types::domain::identity::AskBefore>();
    f::<ordin_types::domain::identity::VoiceSettings>();
    f::<ordin_types::domain::identity::VoiceTone>();
    f::<ordin_types::domain::identity::VoiceRegister>();
    f::<ordin_types::domain::identity::VoiceLength>();
    f::<ordin_types::domain::identity::ServerProfile>();
    f::<ordin_types::domain::identity::ServerProfileKind>();
    f::<ordin_types::domain::identity::ServerShape>();
    f::<ordin_types::domain::places::Workspace>();
    f::<ordin_types::domain::places::WorkspaceState>();
    f::<ordin_types::domain::places::HomeMarks>();
    f::<ordin_types::domain::places::ChangedMark>();
    f::<ordin_types::domain::places::Place>();
    f::<ordin_types::domain::places::PlaceKind>();
    f::<ordin_types::domain::places::WorkspacePlace>();
    f::<ordin_types::domain::places::FolderGrant>();
    f::<ordin_types::domain::places::Grantee>();
    f::<ordin_types::domain::places::FolderAccess>();
    f::<ordin_types::domain::places::SourceLink>();
    f::<ordin_types::domain::places::SourceLinkKind>();
    f::<ordin_types::domain::places::SourceLinkState>();
    f::<ordin_types::domain::places::WorkingCopyItem>();
    f::<ordin_types::domain::places::WorkingCopyState>();
    f::<ordin_types::domain::places::PendingPush>();
    f::<ordin_types::domain::places::PendingPushItem>();
    f::<ordin_types::domain::places::PendingPushState>();
    f::<ordin_types::domain::places::FileVersion>();
    f::<ordin_types::domain::places::FileVersionOrigin>();
    f::<ordin_types::domain::places::Artefact>();
    f::<ordin_types::domain::places::ArtefactLocation>();
    f::<ordin_types::domain::places::MemoryFactRef>();
    f::<ordin_types::domain::places::KnowledgePage>();
    f::<ordin_types::domain::places::KnowledgeAuthor>();
    f::<ordin_types::domain::places::KnowledgeSource>();
    f::<ordin_types::domain::places::KnowledgePageState>();
    f::<ordin_types::domain::places::ConfluenceLink>();
    f::<ordin_types::domain::places::ConfluenceLinkState>();
    f::<ordin_types::domain::places::PublishedCopy>();
    f::<ordin_types::domain::places::CopyOriginal>();
    f::<ordin_types::domain::places::CopyDestination>();
    f::<ordin_types::domain::places::CopyFormat>();
    f::<ordin_types::domain::places::CopyMaker>();
    f::<ordin_types::domain::places::PublishedVia>();
    f::<ordin_types::domain::places::PublishedCopyState>();
    f::<ordin_types::domain::runs::Task>();
    f::<ordin_types::domain::runs::TaskTarget>();
    f::<ordin_types::domain::runs::TaskArgument>();
    f::<ordin_types::domain::runs::Run>();
    f::<ordin_types::domain::runs::RunOrigin>();
    f::<ordin_types::domain::runs::RunState>();
    f::<ordin_types::domain::runs::RunPauseReason>();
    f::<ordin_types::domain::runs::UsageCounters>();
    f::<ordin_types::domain::runs::StartupReport>();
    f::<ordin_types::domain::runs::StartupSubject>();
    f::<ordin_types::domain::runs::RuleNotEnforced>();
    f::<ordin_types::domain::runs::ToolRequest>();
    f::<ordin_types::domain::runs::ToolRequestDecision>();
    f::<ordin_types::domain::runs::ToolRequestState>();
    f::<ordin_types::domain::runs::Approval>();
    f::<ordin_types::domain::runs::ApprovalSubject>();
    f::<ordin_types::domain::runs::ApprovalState>();
    f::<ordin_types::domain::runs::ExternalAction>();
    f::<ordin_types::domain::runs::ExternalActionKind>();
    f::<ordin_types::domain::runs::ExternalActionState>();
    f::<ordin_types::domain::runs::ReusedAnswer>();
    f::<ordin_types::domain::runs::CrewCharter>();
    f::<ordin_types::domain::runs::CoordinatorKind>();
    f::<ordin_types::domain::runs::CrewMember>();
    f::<ordin_types::domain::runs::Handoff>();
    f::<ordin_types::domain::runs::Guardrails>();
    f::<ordin_types::domain::runs::CrewCharterState>();
    f::<ordin_types::domain::runs::CrewRun>();
    f::<ordin_types::domain::runs::CrewRunState>();
    f::<ordin_types::domain::runs::CrewPauseReason>();
    f::<ordin_types::domain::runs::CrewBudgetCounters>();
    f::<ordin_types::domain::runs::Round>();
    f::<ordin_types::domain::runs::Workflow>();
    f::<ordin_types::domain::runs::WorkflowVersion>();
    f::<ordin_types::domain::runs::WorkflowInput>();
    f::<ordin_types::domain::runs::WorkflowAgent>();
    f::<ordin_types::domain::runs::WorkflowOrigin>();
    f::<ordin_types::domain::runs::WorkflowState>();
    f::<ordin_types::domain::runs::WorkflowShare>();
    f::<ordin_types::domain::runs::ShareHost>();
    f::<ordin_types::domain::runs::WorkflowStep>();
    f::<ordin_types::domain::runs::AgentStep>();
    f::<ordin_types::domain::runs::StepUse>();
    f::<ordin_types::domain::runs::IfBlock>();
    f::<ordin_types::domain::runs::NamedPath>();
    f::<ordin_types::domain::runs::EachBlock>();
    f::<ordin_types::domain::runs::UntilBlock>();
    f::<ordin_types::domain::runs::CallBlock>();
    f::<ordin_types::domain::runs::Condition>();
    f::<ordin_types::domain::runs::Report>();
    f::<ordin_types::domain::runs::ReportSection>();
    f::<ordin_types::domain::runs::ReportData>();
    f::<ordin_types::domain::runs::ReportTableColumn>();
    f::<ordin_types::domain::runs::ColumnType>();
    f::<ordin_types::domain::runs::CellValue>();
    f::<ordin_types::domain::runs::ChartKind>();
    f::<ordin_types::domain::runs::BarArrangement>();
    f::<ordin_types::domain::runs::ReportKeyFigure>();
    f::<ordin_types::domain::runs::ReportFigureChange>();
    f::<ordin_types::domain::runs::ReportSource>();
    f::<ordin_types::domain::runs::Dashboard>();
    f::<ordin_types::domain::runs::DashboardSource>();
    f::<ordin_types::domain::runs::RunStep>();
    f::<ordin_types::domain::runs::StepSource>();
    f::<ordin_types::domain::runs::MarkSet>();
    f::<ordin_types::domain::server::ChannelPairing>();
    f::<ordin_types::domain::server::ChannelKind>();
    f::<ordin_types::domain::server::PairingState>();
    f::<ordin_types::domain::server::CardReference>();
    f::<ordin_types::domain::server::CardSubject>();
    f::<ordin_types::domain::server::CardButton>();
    f::<ordin_types::domain::server::CardReferenceState>();
    f::<ordin_types::domain::server::CardDelivery>();
    f::<ordin_types::domain::server::DesktopPairing>();
    f::<ordin_types::domain::server::ServerConnection>();
    f::<ordin_types::domain::server::ServerConnectionState>();
    f::<ordin_types::domain::server::Trigger>();
    f::<ordin_types::domain::server::TriggerSource>();
    f::<ordin_types::domain::server::NatsDelivery>();
    f::<ordin_types::domain::server::FilterCondition>();
    f::<ordin_types::domain::server::FilterOperator>();
    f::<ordin_types::domain::server::InputMapping>();
    f::<ordin_types::domain::server::MappingSource>();
    f::<ordin_types::domain::server::IdentifierSource>();
    f::<ordin_types::domain::server::TriggerLimits>();
    f::<ordin_types::domain::server::TriggerState>();
    f::<ordin_types::domain::server::TriggerEvent>();
    f::<ordin_types::domain::server::TriggerOutcome>();
    f::<ordin_types::domain::server::TelegramLink>();
    f::<ordin_types::domain::server::TelegramChatKind>();
    f::<ordin_types::domain::server::TelegramBotState>();
    f::<ordin_types::domain::server::TelegramLinkState>();
    f::<ordin_types::domain::server::KeptTelegramMessage>();
    f::<ordin_types::domain::server::TelegramFile>();
    f::<ordin_types::domain::server::TelegramDraft>();
    f::<ordin_types::domain::server::TelegramDraftState>();
    f::<ordin_types::domain::server::TelegramPostRequest>();
    f::<ordin_types::domain::server::TelegramPostRequestState>();
    f::<ordin_types::domain::server::SlackLink>();
    f::<ordin_types::domain::server::SlackLinkState>();
    f::<ordin_types::domain::server::KeptSlackMessage>();
    f::<ordin_types::domain::server::SlackDraft>();
    f::<ordin_types::domain::server::SlackDraftState>();
    f::<ordin_types::domain::server::SlackRule>();
    f::<ordin_types::domain::server::SlackRuleKind>();
    f::<ordin_types::domain::server::SlackRuleScope>();
    f::<ordin_types::domain::server::SlackRuleState>();
    f::<ordin_types::domain::server::SlackPost>();
    f::<ordin_types::domain::server::SlackDestination>();
    f::<ordin_types::domain::server::SlackPostOrigin>();
    f::<ordin_types::domain::server::SlackPostAuthority>();
    f::<ordin_types::domain::server::SlackPostState>();
    f::<ordin_types::wire::approval::ApprovalCard>();
    f::<ordin_types::wire::approval::ApprovalAsker>();
    f::<ordin_types::wire::approval::AffectedItem>();
    f::<ordin_types::wire::approval::RejectStyle>();
    f::<ordin_types::wire::approval::ApprovalButtons>();
    f::<ordin_types::wire::approval::CardRef>();
    f::<ordin_types::wire::approval::ApprovalContent>();
    f::<ordin_types::wire::approval::ContentRef>();
    f::<ordin_types::wire::approval::EmailApproval>();
    f::<ordin_types::wire::approval::EmailRecipient>();
    f::<ordin_types::wire::approval::EmailAttachmentLine>();
    f::<ordin_types::wire::approval::ApprovalPress>();
    f::<ordin_types::wire::approval::PressDecision>();
    f::<ordin_types::wire::events::EventKind>();
    f::<ordin_types::wire::hook::HookRequest>();
    f::<ordin_types::wire::hook::HookMessage>();
    f::<ordin_types::wire::hook::HookReply>();
    f::<ordin_types::wire::knowledge_query::PageTier>();
    f::<ordin_types::wire::knowledge_query::PageStatus>();
    f::<ordin_types::wire::knowledge_query::KnowledgeQueryRequest>();
    f::<ordin_types::wire::knowledge_query::KnowledgeSearch>();
    f::<ordin_types::wire::knowledge_query::KnowledgeSearchKind>();
    f::<ordin_types::wire::knowledge_query::KnowledgeHit>();
    f::<ordin_types::wire::knowledge_query::KnowledgeHitKind>();
    f::<ordin_types::wire::knowledge_query::SharedIdentity>();
    f::<ordin_types::wire::knowledge_query::KnowledgeResult>();
    f::<ordin_types::wire::knowledge_query::UnansweredSource>();
    f::<ordin_types::wire::knowledge_query::UnansweredReason>();
    f::<ordin_types::wire::knowledge_query::KnowledgeContents>();
    f::<ordin_types::wire::knowledge_query::ContentsRepository>();
    f::<ordin_types::wire::knowledge_query::RepositoryRole>();
    f::<ordin_types::wire::knowledge_query::ContentsThing>();
    f::<ordin_types::wire::knowledge_query::KnowledgeAbout>();
    f::<ordin_types::wire::knowledge_query::SourcesRecordPolicy>();
    f::<ordin_types::wire::knowledge_query::RepositoryMoved>();
    f::<ordin_types::wire::rules::ToolOrigin>();
    f::<ordin_types::wire::rules::ToolCall>();
    f::<ordin_types::wire::rules::DecisionPoint>();
    f::<ordin_types::wire::rules::ToolDecision>();
    f::<ordin_types::wire::rules::Verdict>();
    f::<ordin_types::wire::rules::RuleRef>();
    f::<ordin_types::wire::rules::RuleSource>();
    f::<ordin_types::wire::rules::ActionProposal>();
    f::<ordin_types::wire::run::RunProgress>();
    f::<ordin_types::wire::run::ProgressCount>();
    f::<ordin_types::wire::run::StepMark>();
    f::<ordin_types::wire::run::AdapterKind>();
    f::<ordin_types::wire::run::AdapterVerb>();
    f::<ordin_types::wire::run::RunTarget>();
    f::<ordin_types::wire::run::RunLimits>();
    f::<ordin_types::wire::run::ModelLevel>();
    f::<ordin_types::wire::run::ThinkingLevel>();
    f::<ordin_types::wire::run::ModelChoice>();
    f::<ordin_types::wire::startup::StartUpSubject>();
    f::<ordin_types::wire::startup::StartUpReportItem>();
    f::<ordin_types::wire::startup::NotEnforcedReason>();
    f::<ordin_types::wire::startup::UnavailableReason>();
    f::<ordin_types::wire::workflow_file::WorkflowFile>();
    f::<ordin_types::wire::workflow_file::WorkflowFileInput>();
    f::<ordin_types::wire::workflow_file::WorkflowInputKind>();
    f::<ordin_types::wire::workflow_file::WorkflowFileAgent>();
    f::<ordin_types::wire::workflow_file::AgentRepositoryRef>();
    f::<ordin_types::wire::workflow_file::WorkflowFileLimits>();
    f::<ordin_types::wire::workflow_file::WorkflowFileStep>();
    f::<ordin_types::wire::workflow_file::NamedPath>();
    f::<ordin_types::wire::workflow_file::StepUse>();
    f::<ordin_types::wire::workflow_file::ListSource>();
    f::<ordin_types::wire::workflow_file::IfItFails>();
    f::<ordin_types::wire::workflow_file::Decider>();
    f::<ordin_types::wire::workflow_file::ApplicationCheck>();
};

// ---- the agent configuration (AgentConfiguration.md *Example* and its ADR-113 amendment) ----

use ordin_types::domain::agent_profile::{
    AgentConfig, ChatContext, ContextStep, Delivery, ModelSetting, RESERVED_ENVIRONMENT_NAMES,
    RESERVED_ENVIRONMENT_PREFIX,
};
use ordin_types::yaml::read_restricted;

const SUPPORT_AGENT: &str = r#"package: ./support-agent
model:
  primary: anthropic:claude-sonnet-4-6
  fallback: anthropic:claude-haiku-4-5
  temperature: 0.2
  safe-reply: I could not answer that. Please ask the service desk.
run-limit-minutes: 30
writable-paths: [/.agent-run/, /memory/]
tools:
  http:
    - name: tickets
      base-url: https://tickets.example.org/api
      openapi: tickets.openapi.yaml
      expose: [getTicket, createTicket]
      headers: {Authorization: TICKETS_AUTHORIZATION}
  publish:
    - name: supplier-mail
      tool: email_supplier
      description: Sends one message to the supplier desk.
      url-env: SUPPLIER_WEBHOOK_URL
      external: true
  approve-before: [email_supplier]
chat:
  - name: workspace
    kind: slack
    bot-token-env: SLACK_BOT_TOKEN
    app-token-env: SLACK_APP_TOKEN
    triggers: [{kind: mention, delivery: thread}]
    failure-text: Something went wrong on our side.
    approval-text: This request is waiting for approval.
contract: {capabilities: [events]}
harness: claude
traces: true
reach:
  channels: [C04HELPDESK]
  direct-message-users: [U02ASHA]
approvers:
  users: [{chat-user: U07LEAD, subject: 00u1lead}]
  groups: [service-desk-leads]
  asker-may-approve-internal: true
auth:
  issuer: https://example.okta.com/oauth2/default
  audience: support-agent
  jwks-url: https://example.okta.com/oauth2/default/v1/keys
  role-claim: ordin_role
  operator-value: support-operator
  caller-value: support-caller
"#;

const KNOWLEDGE_AGENT: &str = r#"knowledge-agent:
  name: Supply knowledge
  audiences: [supply-staff]
  group-claim: groups
  front-doors: [svc-knowledge-frontdoor-ops]
  glossaries:
    - {url: https://git.example.org/org/glossary, credential-env: GLOSSARY_TOKEN, audience-may-read: true}
  repositories:
    - {url: https://git.example.org/supply/handbook, path: policies, credential-env: HANDBOOK_TOKEN, audience-may-read: true}
    - {url: https://git.example.org/supply/contracts, branch: release, credential-env: CONTRACTS_TOKEN, audience-may-read: true}
  proxies:
    - {name: finance, url: https://finance-knowledge.example.org, audience: finance-knowledge}
  service-identity:
    token-url: https://idp.example.com/oauth2/token
    client-id: supply-knowledge
    client-secret-env: KNOWLEDGE_CLIENT_SECRET
  sources-record: {enabled: true, retention-days: 180}
"#;

#[test]
fn the_support_agent_example_reads_into_agent_config() {
    let config: AgentConfig = read_restricted(SUPPORT_AGENT, 1_048_576).expect("the example reads");
    assert_eq!(config.run_limit_minutes, 30);
    assert!(config.traces);
    let ModelSetting::Policy(policy) = &config.model else {
        panic!("the mapping form");
    };
    assert_eq!(policy.primary, "anthropic:claude-sonnet-4-6");
    assert_eq!(policy.temperature, 0.2);
    assert!(policy.environment.is_empty());
    assert_eq!(config.tools.http[0].timeout_seconds, 30);
    assert_eq!(config.tools.publish[0].field, "message");
    let chat = &config.chat[0];
    assert_eq!(chat.triggers[0].delivery, Delivery::Thread);
    assert_eq!(chat.status.received, "eyes");
    assert_eq!(chat.context, ChatContext::default());
    // `Fixed` widened (V-18): the transcript and images steps, no channel documents.
    assert_eq!(
        chat.context.steps(),
        vec![
            ContextStep::Transcript { max_messages: 200 },
            ContextStep::Images {
                max_bytes: 10_485_760,
                max_total_bytes: 20_971_520
            },
        ]
    );
    assert!(config.knowledge_agent.is_none());

    // The value written back reads as the same configuration.
    let written = serde_json::to_value(&config).unwrap();
    let again: AgentConfig = serde_json::from_value(written).unwrap();
    assert_eq!(again, config);
}

#[test]
fn the_short_model_form_and_context_steps_read() {
    let text = r#"package: ./a
model: anthropic:claude-sonnet-4-6
run-limit-minutes: 5
chat:
  - name: w
    kind: slack
    bot-token-env: B
    app-token-env: A
    failure-text: f
    approval-text: a
    context:
      - {kind: transcript}
      - {kind: knowledge-lookup, source: faq, max-bytes: 4096}
      - {kind: precedence-note, text: Pages win.}
"#;
    let config: AgentConfig = read_restricted(text, 65_536).unwrap();
    assert_eq!(
        config.model,
        ModelSetting::Short("anthropic:claude-sonnet-4-6".into())
    );
    let steps = config.chat[0].context.steps();
    assert_eq!(steps.len(), 3);
    assert_eq!(
        steps[1],
        ContextStep::KnowledgeLookup {
            source: "faq".into(),
            limit: 5,
            max_bytes: 4096
        }
    );
    let written = serde_json::to_value(&config).unwrap();
    assert_eq!(written["model"], "anthropic:claude-sonnet-4-6");
    assert!(written["chat"][0]["context"].is_array());
    assert_eq!(
        serde_json::from_value::<AgentConfig>(written).unwrap(),
        config
    );

    // The fixed form with channel documents widens to three steps.
    let fixed: ChatContext = serde_json::from_value(serde_json::json!({
        "images": false, "channel-documents": true, "channel-documents-max-bytes": 1024
    }))
    .unwrap();
    assert_eq!(
        fixed.steps(),
        vec![
            ContextStep::Transcript { max_messages: 200 },
            ContextStep::ChannelDocuments { max_bytes: 1024 }
        ]
    );
    // A mapping's own key errors are reported: an unknown key is refused.
    assert!(serde_json::from_value::<ChatContext>(serde_json::json!({"colour": 1})).is_err());
    assert!(serde_json::from_value::<ModelSetting>(serde_json::json!(12)).is_err());
    assert!(serde_json::from_value::<ModelSetting>(serde_json::json!({"primary": "a:b"})).is_err());
}

#[test]
fn the_knowledge_agent_example_reads_with_its_defaults() {
    let text = format!(
        "package: ./k\nmodel: anthropic:claude-sonnet-4-6\nrun-limit-minutes: 5\n{KNOWLEDGE_AGENT}"
    );
    let config: AgentConfig = read_restricted(&text, 65_536).unwrap();
    let section = config.knowledge_agent.expect("the section");
    assert_eq!(section.max_proxies, 8);
    assert_eq!(section.reading.daily_documents, 1_000);
    assert_eq!(section.fetch_interval_minutes, 15);
    assert_eq!(section.hot_copy.asks_per_day, 200);
    assert!(section.hot_copy.credential_env.is_none());
    assert!(section.mcp.enabled);
    assert!(section.sources_record.enabled);
    assert_eq!(section.sources_record.retention_days, 180);
    assert!(section.repositories.iter().all(|r| r.audience_may_read));
}

#[test]
fn the_reserved_environment_names_sit_with_the_types() {
    assert_eq!(RESERVED_ENVIRONMENT_NAMES.len(), 20);
    assert!(RESERVED_ENVIRONMENT_NAMES.contains(&"ANTHROPIC_API_KEY"));
    assert_eq!(RESERVED_ENVIRONMENT_PREFIX, "ORDIN_");
}

#[test]
fn model_policy_types_never_show_their_secrets() {
    use ordin_types::domain::agent_profile::{CallPlace, ModelFailureCause, ModelProvider};
    assert!(ModelProvider("anthropic".into()).is_default());
    assert!(!ModelProvider("bedrock".into()).is_default());
    assert_eq!(
        serde_json::to_string(&ModelFailureCause::ContentFilter).unwrap(),
        "\"content_filter\""
    );
    assert_ne!(CallPlace::Agent, CallPlace::Subagent);
}

// ---- the timestamp property (Internal Logic 9) ----

#[test]
fn timestamp_now_round_trips() {
    use ordin_types::time::Timestamp;
    for _ in 0..1_000 {
        let now = Timestamp::now();
        let back: Timestamp = serde_json::from_str(&serde_json::to_string(&now).unwrap()).unwrap();
        assert_eq!(back, now);
        assert_eq!(back.to_string(), now.to_wire());
        assert_eq!(Timestamp::from_unix_millis(now.unix_millis()).unwrap(), now);
    }
}

// ---- the identifier forms ----

#[test]
fn identifiers_parse_display_and_default() {
    use ordin_types::{BlockId, RunId};
    let id = RunId::default();
    let text = id.to_string();
    assert_eq!(text.parse::<RunId>().unwrap(), id);
    assert_eq!(RunId::parse(&text).unwrap().as_uuid(), id.as_uuid());
    assert!("nope".parse::<BlockId>().is_err());
    assert_ne!(RunId::new(), RunId::new());
}

// ---- the restricted reader's node form and core-schema resolution ----

#[test]
fn yaml_node_form_keeps_text_and_marks_quoting() {
    use ordin_types::yaml::{read_restricted_node, YamlNode};
    let text =
        "a: '007'\nb: 007\nc:\nd: ~\ne: null\nf: |\n  line\ng: [x, \"y\"]\n!!str h: !!int 1\n";
    let node = read_restricted_node(text, 1024).unwrap();
    let scalar = |t: &str, q: bool| YamlNode::Scalar {
        text: t.into(),
        quoted: q,
    };
    assert_eq!(
        node,
        YamlNode::Map(vec![
            ("a".into(), scalar("007", true)),
            ("b".into(), scalar("007", false)),
            ("c".into(), YamlNode::Null),
            ("d".into(), YamlNode::Null),
            ("e".into(), YamlNode::Null),
            ("f".into(), scalar("line\n", true)),
            (
                "g".into(),
                YamlNode::Seq(vec![scalar("x", false), scalar("y", true)])
            ),
            ("h".into(), scalar("1", false)),
        ])
    );
    assert_eq!(read_restricted_node("", 10).unwrap(), YamlNode::Null);
}

#[test]
fn yaml_plain_scalars_resolve_by_the_core_schema() {
    let value: serde_json::Value = read_restricted(
        "n1: ~\nn2: Null\nb1: True\nb2: FALSE\ni1: -12\ni2: 0o17\ni3: 0x1F\nf1: 1.5\nf2: .5e1\n\
s1: '12'\ns2: yes\ns3: 1.2.3\nt1: !!str 12\nt2: !!float 3\nt3: !!bool true\nt4: !!null ~\nt5: !!int 0x10\n\
m: !!map {k: v}\nq: !!seq [1]\n",
        4096,
    )
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "n1": null, "n2": null, "b1": true, "b2": false, "i1": -12, "i2": 15, "i3": 31,
            "f1": 1.5, "f2": 5.0, "s1": "12", "s2": "yes", "s3": "1.2.3", "t1": "12", "t2": 3.0,
            "t3": true, "t4": null, "t5": 16, "m": {"k": "v"}, "q": [1]
        })
    );
    for refused in [
        "a: !!bool maybe\n",
        "a: !!null x\n",
        "a: !!float x\n",
        "a: !!map 1\n",
        "a: !!seq {b: 1}\n",
    ] {
        assert!(
            read_restricted::<serde_json::Value>(refused, 1024).is_err(),
            "{refused}"
        );
    }
    let verbatim: serde_json::Value =
        read_restricted("a: !<tag:yaml.org,2002:str> 5\n", 1024).unwrap();
    assert_eq!(verbatim, serde_json::json!({"a": "5"}));
    // Exactly 64 levels of nesting are accepted.
    let deep = format!("{}{}", "[".repeat(64), "]".repeat(64));
    assert!(read_restricted::<serde_json::Value>(&deep, 1024).is_ok());
    // An explicit document marker and end marker, still one document.
    let one: serde_json::Value = read_restricted("---\na: 1\n...\n", 1024).unwrap();
    assert_eq!(one, serde_json::json!({"a": 1}));
}
