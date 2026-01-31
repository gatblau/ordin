//! X2 for the amendments at the end of ServiceApi.md (ADR-102 to ADR-114): one test per added
//! scenario, named after it (its exact title in the comment above it). A count a scenario states
//! before a later amendment is asserted at today's figure, 400 rows (ADR-114).

mod support;

use ordin_service_api::blocks::*;
use ordin_service_api::*;
use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde_json::{json, Value};
use support::*;

const WS: &str = "0192a0f1-1111-7222-8333-444455556666";
const OTHER: &str = "0192a0f1-2222-7333-8444-555566667777";

fn assert_in_catalogue(names: &[&str]) {
    for name in names {
        assert!(command(name).is_some(), "{name} in the catalogue");
    }
    assert_eq!(CATALOGUE.len(), 415);
}

fn refused(command_name: &str, input: Value) -> Refusal {
    accept_local(command_name, input).expect_err("refused")
}

// ---- ADR-102 ----

// Scenario: Happy path — the Slack group is in the catalogue
#[test]
fn happy_path_the_slack_group_is_in_the_catalogue() {
    assert_in_catalogue(&[
        "slack.status",
        "slack.link",
        "slack.add_rule",
        "handover.open_link",
        "workspaces.set_slack_grant",
    ]);
}

struct OneLinkElsewhere(SlackLinkId);

#[async_trait::async_trait]
impl RefResolver for OneLinkElsewhere {
    async fn content(&self, _: WorkspaceId, _: &ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        true
    }
    async fn entity(&self, _: WorkspaceId, entity: EntityRef) -> bool {
        entity != EntityRef::SlackLink(self.0)
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        None
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        PlanState::default()
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

// Scenario: Edge case — a Slack list block for another workspace's link
#[tokio::test]
async fn edge_case_a_slack_list_block_for_another_workspaces_link() {
    let hr_link = SlackLinkId::new();
    let ctx = BlockCheckContext {
        workspace_id: WorkspaceId::new(),
        run_id: None,
        now: Timestamp::now(),
    };
    let raw = json!({"kind": "slack_list", "slack_link_id": hr_link});
    let rejections = check_block(&raw, &ctx, &OneLinkElsewhere(hr_link))
        .await
        .unwrap_err();
    assert_eq!(rejections[0].code, "BLOCK_REF_UNRESOLVED");
    assert_eq!(rejections[0].at, "/slack_link_id");
}

fn slack_rule() -> Value {
    json!({"rule": {"kind": "ordin_answers", "scope": {"kind": "link", "slack_link_id": SlackLinkId::new()}, "channel_id": "C0123456789", "channel_name": "q3-suppliers", "daily_cap": 20}})
}

// Scenario: Error path — an unknown field in slack.add_rule
#[test]
fn error_path_an_unknown_field_in_slack_add_rule() {
    assert!(accept_local("slack.add_rule", slack_rule()).is_ok());
    let mut input = slack_rule();
    input["post_now"] = json!(true);
    assert_eq!(refused("slack.add_rule", input).code, "SVC_INPUT_INVALID");
}

// ---- ADR-103 ----

// Scenario: Happy path — the team knowledge group is in the catalogue
#[test]
fn happy_path_the_team_knowledge_group_is_in_the_catalogue() {
    assert_in_catalogue(&[
        "team_knowledge.status",
        "team_knowledge.turn_on",
        "team_knowledge.get",
        "team_knowledge.share",
        "memory.keep_checked",
    ]);
}

// Scenario: Error path — an unknown choice kind in team_knowledge.turn_on
#[test]
fn error_path_an_unknown_choice_kind_in_team_knowledge_turn_on() {
    let input = json!({"ws": WS, "choice": {"kind": "local_folder", "path": "/tmp/x"}});
    assert_eq!(
        refused("team_knowledge.turn_on", input).code,
        "SVC_INPUT_INVALID"
    );
}

// Scenario: Edge case — team knowledge over the Remote API
#[test]
fn edge_case_team_knowledge_over_the_remote_api() {
    let meta = command("team_knowledge.status").unwrap();
    assert_eq!(meta.rest_path, "/v1/team_knowledge/status");
    let accepted = accept_with(
        &envelope("team_knowledge.status", json!({"ws": WS})),
        "team_knowledge.status",
        paired(),
    )
    .unwrap();
    assert_eq!(accepted.meta.name, "team_knowledge.status");
}

// ---- ADR-104 ----

// Scenario: Happy path — the suggestion commands are in the catalogue
#[test]
fn happy_path_the_suggestion_commands_are_in_the_catalogue() {
    assert_in_catalogue(&[
        "knowledge.accept_suggestion",
        "knowledge.dismiss_suggestion",
    ]);
}

fn page_edit() -> Value {
    json!({"page_id": KnowledgePageId::new(), "title": "Supplier rates", "body_markdown": "Q3 rates."})
}

// Scenario: Edge case — save_edit without the new fields
#[test]
fn edge_case_save_edit_without_the_new_fields() {
    let accepted = accept_local("knowledge.save_edit", page_edit()).unwrap();
    assert!(accepted.input.get("tags").is_none_or(Value::is_null));
    assert!(accepted.input.get("aliases").is_none_or(Value::is_null));
}

// Scenario: Error path — a suggestion id that is not a UUID
#[test]
fn error_path_a_suggestion_id_that_is_not_a_uuid() {
    let input = json!({"page_id": KnowledgePageId::new(), "suggestion_id": "abc"});
    assert_eq!(
        refused("knowledge.accept_suggestion", input).code,
        "SVC_INPUT_INVALID"
    );
}

// ---- ADR-106 ----

// Scenario: Happy path — the Jira group is in the catalogue
#[test]
fn happy_path_the_jira_group_is_in_the_catalogue() {
    assert_in_catalogue(&["jira.connect_token", "jira.find_issues"]);
    assert!(command("jira.find_assignable").is_none());
}

// Scenario: Error path — a Jira command over the Remote API
#[test]
fn error_path_a_jira_command_over_the_remote_api() {
    let body = envelope("jira.find_issues", json!({"text": "rates", "limit": 10}));
    assert_eq!(
        accept_with(&body, "jira.find_issues", paired())
            .unwrap_err()
            .code,
        "SVC_COMMAND_LOCAL_ONLY"
    );
}

// Scenario: Edge case — the token is not echoed
#[tokio::test]
async fn edge_case_the_token_is_not_echoed() {
    let token = "ATATT3xFfGF0-secret-token-value";
    let api = Mock::answering(json!({}));
    let input = json!({"site": "acme.atlassian.net", "email": "x", "token": token});
    let (reply, json) = handle_local(&api, "jira.connect_token", input).await;
    assert_eq!(json["error"]["code"], "SVC_INPUT_INVALID");
    assert!(!reply.json.contains("secret"));
    assert!(api.calls().is_empty());
}

// Scenario: Error path — an unknown issue selection kind
#[test]
fn error_path_an_unknown_issue_selection_kind() {
    let input = json!({"draft": "0192a0f1-1111-7222-8333-444455556666", "jira_issue": {"kind": "pick_any"}});
    assert_eq!(
        refused("handover.update_draft", input).code,
        "SVC_INPUT_INVALID"
    );
}

// ---- ADR-109 ----

// Scenario: Happy path — the language commands are in the catalogue
#[test]
fn happy_path_the_language_commands_are_in_the_catalogue() {
    assert_in_catalogue(&[
        "localisation.list_languages",
        "localisation.edit_message",
        "localisation.import_language",
        "ordin.draft_language",
    ]);
}

// Scenario: Happy path — language_changed carries the owner's payload
#[test]
fn happy_path_language_changed_carries_the_owners_payload() {
    let event = ServiceEvent::from_journal(
        "language_changed",
        json!({"locale": "ja-JP", "change": "draft_progress"}),
    )
    .unwrap();
    let ServiceEvent::LanguageChanged { locale, change } = event else {
        panic!("language_changed");
    };
    assert_eq!(locale, "ja-JP");
    assert_eq!(serde_json::to_value(change).unwrap(), "draft_progress");
}

// Scenario: Error path — an import larger than the limit
#[tokio::test]
async fn error_path_an_import_larger_than_the_limit() {
    // The 9 MiB body admits the file; Localisation refuses it with its own code, unchanged.
    let api = Mock::new(|_, _| Box::pin(async { Err(module_error("LOC_FILE_TOO_LARGE")) }));
    let input = json!({"file_name": "fr.ftl", "ftl": "a".repeat(2_097_153), "replace": false});
    let (_, json) = handle_local(&api, "localisation.import_language", input).await;
    assert_eq!(json["error"]["code"], "LOC_FILE_TOO_LARGE");
    assert_eq!(api.calls(), ["localisation_import_language"]);
}

// Scenario: Error path — an unknown filter value
#[test]
fn error_path_an_unknown_filter_value() {
    let input = json!({"locale": "fr-FR", "only": "everything"});
    assert_eq!(
        refused("localisation.list_messages", input).code,
        "SVC_INPUT_INVALID"
    );
}

// Scenario: Edge case — the existing replies keep their shape
#[test]
fn edge_case_the_existing_replies_keep_their_shape() {
    let schema = json_schemas();
    let result = schema["$defs"]["LocalisationGetSettingsResult"]["properties"]
        .as_object()
        .unwrap();
    let keys: Vec<&str> = result.keys().map(String::as_str).collect();
    assert_eq!(keys, ["active", "following_operating_system", "offered"]);
}

// ---- ADR-110 ----

// Scenario: Happy path — the new commands are in the catalogue
#[test]
fn happy_path_the_new_commands_are_in_the_catalogue() {
    assert_in_catalogue(&[
        "team_knowledge.add_read_folder",
        "team_knowledge.stop_reading",
        "team_knowledge.adopt",
        "team_knowledge.adopt_again",
        "team_knowledge.keep_as_is",
        "team_knowledge.set_description",
        "team_knowledge.check_recall",
        "knowledge.list_read_pages",
    ]);
}

// Scenario: Edge case — an old get input is still valid
#[test]
fn edge_case_an_old_get_input_is_still_valid() {
    let accepted = accept_local("team_knowledge.get", json!({"ws": WS})).unwrap();
    assert!(accepted.input.get("folder_id").is_none_or(Value::is_null));
}

// Scenario: Error path — an unknown adopt item kind
#[test]
fn error_path_an_unknown_adopt_item_kind() {
    let input = json!({"ws": WS, "item": {"kind": "relationship", "id": "0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b"}});
    let refusal = refused("team_knowledge.adopt", input);
    assert_eq!(
        (refusal.code, refusal.at.as_deref()),
        ("SVC_INPUT_INVALID", Some("/input/item"))
    );
}

/// Every name a run can reach in this crate: the block catalogue and the tool arguments, as schemas.
fn agent_reachable_text() -> String {
    let schema = json_schemas();
    let defs = &schema["$defs"];
    ["Block", "PlanToolArgs", "ReportToolArgs"]
        .iter()
        .map(|name| defs[*name].to_string())
        .collect()
}

fn assert_person_only(names: &[&str]) {
    let reachable = agent_reachable_text();
    for name in names {
        assert!(PERSON_ONLY_COMMANDS.contains(name), "{name} person-only");
        assert!(!reachable.contains(name), "{name} named by a block or tool");
        let ipc = name.replace('.', "_");
        assert!(!reachable.contains(&ipc), "{ipc} named by a block or tool");
    }
}

// Scenario: Edge case — adopting is the person's alone
#[test]
fn edge_case_adopting_is_the_persons_alone() {
    assert_person_only(&[
        "team_knowledge.adopt",
        "team_knowledge.adopt_again",
        "team_knowledge.keep_as_is",
    ]);
}

// Scenario: Edge case — adopting over the Remote API
#[test]
fn edge_case_adopting_over_the_remote_api() {
    let input = json!({"ws": WS, "item": {"kind": "fact", "id": MemoryFactId::new()}});
    let accepted = accept_with(
        &envelope("team_knowledge.adopt", input),
        "team_knowledge.adopt",
        paired(),
    )
    .unwrap();
    assert_eq!(accepted.meta.rest_path, "/v1/team_knowledge/adopt");
}

// ---- ADR-111 ----

// Scenario: Happy path — the new memory commands are in the catalogue
#[test]
fn happy_path_the_new_memory_commands_are_in_the_catalogue() {
    assert_in_catalogue(&[
        "memory.things",
        "memory.thing",
        "memory.fact_history",
        "memory.fact_origins",
        "memory.keep",
        "memory.used",
        "memory.remembered_in_run",
        "memory.correct_link",
        "memory.forget_link",
        "memory.thing_count",
        "memory.forget_thing",
        "memory.same_as",
        "memory.forget_count",
    ]);
}

// Scenario: Error path — both a workspace and a scope
#[test]
fn error_path_both_a_workspace_and_a_scope() {
    let refusal = refused(
        "memory.things",
        json!({"workspace_id": WS, "scope_id": OTHER}),
    );
    assert_eq!(
        (refusal.code, refusal.at.as_deref()),
        ("SVC_INPUT_INVALID", Some("/input"))
    );
}

// Scenario: Error path — a malformed thing id
#[test]
fn error_path_a_malformed_thing_id() {
    let refusal = refused(
        "memory.thing",
        json!({"workspace_id": WS, "id": "Borealis"}),
    );
    assert_eq!(
        (refusal.code, refusal.at.as_deref()),
        ("SVC_INPUT_INVALID", Some("/input/id"))
    );
}

// Scenario: Edge case — changing memory is the person's alone
#[test]
fn edge_case_changing_memory_is_the_persons_alone() {
    assert_person_only(&[
        "memory.keep",
        "memory.correct_link",
        "memory.forget_link",
        "memory.forget_thing",
        "memory.same_as",
    ]);
}

// Scenario: Edge case — an answer names the recalls it used
#[test]
fn edge_case_an_answer_names_the_recalls_it_used() {
    let (r1, r2) = (MemoryRecallId::new(), MemoryRecallId::new());
    let block = MessageBlock {
        text: "Answer".into(),
        attachments: vec![],
        memory_used: vec![r1, r2],
    };
    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["memory_used"], json!([r1, r2]));
    let empty = MessageBlock {
        text: "Answer".into(),
        attachments: vec![],
        memory_used: vec![],
    };
    assert!(serde_json::to_value(&empty)
        .unwrap()
        .get("memory_used")
        .is_none());
}

// ---- ADR-112, ADR-113 ----

// Scenario: Happy path — the knowledge agents commands are in the catalogue
#[test]
fn happy_path_the_knowledge_agents_commands_are_in_the_catalogue() {
    assert_in_catalogue(&[
        "knowledge_agents.list",
        "knowledge_agents.add",
        "knowledge_agents.remove",
        "knowledge_agents.details",
        "memory.more_found",
    ]);
}

// Scenario: Error path — a front door address that is not https
#[tokio::test]
async fn error_path_a_front_door_address_that_is_not_https() {
    let api = Mock::answering(json!({}));
    let input = json!({"workspace_id": WS, "url": "http://knowledge.example.org"});
    let refusal = refused("knowledge_agents.add", input.clone());
    assert_eq!(
        (refusal.code, refusal.at.as_deref()),
        ("SVC_INPUT_INVALID", Some("/input/url"))
    );
    let _ = handle_local(&api, "knowledge_agents.add", input).await;
    assert!(api.calls().is_empty());
}

// Scenario: Edge case — connecting a front door is the person's alone
#[test]
fn edge_case_connecting_a_front_door_is_the_persons_alone() {
    assert_person_only(&["knowledge_agents.add", "knowledge_agents.remove"]);
}

// Scenario: Edge case — an old page edit is still valid
#[test]
fn edge_case_an_old_page_edit_is_still_valid() {
    let accepted = accept_local("knowledge.save_edit", page_edit()).unwrap();
    assert!(accepted.input.get("type").is_none_or(Value::is_null));
    assert!(accepted.input.get("description").is_none_or(Value::is_null));
}

// Scenario: Happy path — late results are announced without their content
#[test]
fn happy_path_late_results_are_announced_without_their_content() {
    let recall = MemoryRecallId::new();
    let data = json!({"workspace_id": WS, "recall_id": recall, "added": 3, "unanswered": []});
    let event = ServiceEvent::from_journal("more_found", data).unwrap();
    assert_eq!(event.kind(), EventKind::MoreFound);
    let ServiceEvent::MoreFound {
        workspace_id,
        recall_id,
        added,
        ..
    } = &event
    else {
        panic!("more_found");
    };
    assert_eq!(workspace_id.to_string(), WS);
    assert_eq!(*recall_id, recall);
    assert_eq!(*added, 3);
    // The data holds the ids, the count and the unanswered list: no query text and no statement.
    let mut keys: Vec<String> = event
        .to_journal_data()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    assert_eq!(keys, ["added", "recall_id", "unanswered", "workspace_id"]);
}

// Event table data limits: `added` 1–50, `unanswered` 0–8 entries with `agent` 1–200 characters.
#[test]
fn error_path_more_found_outside_its_data_limits() {
    let base = |added: u16, unanswered: Value| json!({"workspace_id": WS, "recall_id": MemoryRecallId::new(), "added": added, "unanswered": unanswered});
    for data in [
        base(0, json!([])),
        base(51, json!([])),
        base(
            1,
            json!(vec![json!({"agent": "a", "reason": "refused"}); 9]),
        ),
        base(1, json!([{"agent": "", "reason": "not_in_time"}])),
    ] {
        assert_eq!(
            ServiceEvent::from_journal("more_found", data)
                .unwrap_err()
                .code,
            "SVC_EVENT_INVALID"
        );
    }
}

// The front doors of a workspace changed: the screens re-read `knowledge_agents.list`.
#[test]
fn happy_path_knowledge_agents_changed_names_the_workspace() {
    let event = ServiceEvent::from_journal("knowledge_agents_changed", json!({"workspace_id": WS}))
        .unwrap();
    assert_eq!(event.kind(), EventKind::KnowledgeAgentsChanged);
    assert_eq!(event.to_journal_data(), json!({"workspace_id": WS}));
}

// ---- ADR-114 ----

// Scenario: Happy path — the update commands are in the catalogue
#[test]
fn happy_path_the_update_commands_are_in_the_catalogue() {
    assert_in_catalogue(&[
        "setup.check_update",
        "setup.whats_new",
        "setup.whats_new_seen",
        "setup.server_update",
        "setup.update_server",
    ]);
    // "39 groups" counts the catalogue's sections; the `CommandMeta.module` names are 48.
    assert_eq!(every_group().len(), 48);
}

fn no_switch_desktop(answer: Value) -> Mock {
    let mut api = Mock::answering(answer);
    api.groups = ALWAYS_ANSWERED_GROUPS_SORTED.get_or_init(sorted_always);
    api
}

static ALWAYS_ANSWERED_GROUPS_SORTED: std::sync::OnceLock<&'static [&'static str]> =
    std::sync::OnceLock::new();

fn sorted_always() -> &'static [&'static str] {
    let mut groups = ALWAYS_ANSWERED_GROUPS.to_vec();
    groups.sort_unstable();
    Box::leak(groups.into_boxed_slice())
}

// Scenario: Happy path — the greeting names the groups this build answers
#[tokio::test]
async fn happy_path_the_greeting_names_the_groups_this_build_answers() {
    let groups = sorted_always();
    let hello = json!({"api_version": {"major": 1, "minor": 0}, "backend": BACKEND, "backend_name": "This computer", "profile": "desktop", "last_seq": 0, "groups": groups});
    let api = no_switch_desktop(hello);
    let (_, json) = handle_local(&api, "service.hello", json!({})).await;
    let listed: Vec<&str> = json["result"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g.as_str().unwrap())
        .collect();
    assert!(listed.contains(&"ordin") && listed.contains(&"setup"));
    for absent in ["mail", "dictation", "knowledge_agents"] {
        assert!(!listed.contains(&absent), "{absent}");
    }
    assert!(listed.windows(2).all(|w| w[0] < w[1]), "sorted, no repeats");
    // SVC-RL-5: the groups are module names of the catalogue, and service.hello returns them unchanged.
    assert_eq!(listed, api.groups);
    for group in api.groups {
        assert!(CATALOGUE.iter().any(|m| m.module == *group), "{group}");
    }
}

// Scenario: Error path — a command whose group the build lacks
#[tokio::test]
async fn error_path_a_command_whose_group_the_build_lacks() {
    let api = no_switch_desktop(json!({}));
    let (reply, json) = handle_local(&api, "mail.list_messages", json!({})).await;
    assert_eq!(json["error"]["code"], "SVC_NOT_IN_THIS_VERSION");
    assert_eq!(reply.http_status, 404);
    assert!(api.calls().is_empty());
}

// Scenario: Edge case — an absent group is refused before the local-only rule
#[tokio::test]
async fn edge_case_an_absent_group_is_refused_before_the_local_only_rule() {
    let api = no_switch_desktop(json!({}));
    let body = envelope("dictation.start", json!({"field": "message"}));
    let (_, json) = handle_with(&api, &body, "dictation.start", paired(), 10).await;
    assert_eq!(json["error"]["code"], "SVC_NOT_IN_THIS_VERSION");
}

// Scenario: Edge case — an unknown command is still unknown
#[tokio::test]
async fn edge_case_an_unknown_command_is_still_unknown() {
    let api = no_switch_desktop(json!({}));
    let (_, json) = handle_local(&api, "mail.no_such_verb", json!({})).await;
    assert_eq!(json["error"]["code"], "SVC_COMMAND_UNKNOWN");
}

// Scenario: Edge case — starting an update is the person's alone
#[test]
fn edge_case_starting_an_update_is_the_persons_alone() {
    assert_person_only(&["setup.update_server", "setup.install_update"]);
}

// Scenario: Edge case — an older desktop reads a newer greeting
#[test]
fn edge_case_an_older_desktop_reads_a_newer_greeting() {
    let files = ordin_service_api::typescript::generate();
    let types = &files.iter().find(|(n, _)| *n == "types.gen.ts").unwrap().1;
    let hello = &types[types.find("export interface ServiceHello {").unwrap()..];
    let hello = &hello[..hello.find("\n}").unwrap()];
    assert!(hello.contains("  groups: string[];"), "{hello}");
}

// ---- Commands the screens wait on (SVC-SCR-1 to SVC-SCR-6) ----

// Scenario: Contract — the catalogue count
#[test]
fn contract_the_catalogue_count_with_the_screens_commands() {
    assert_in_catalogue(&[
        "files.read_version",
        "memory.held_writes",
        "memory.keep_held",
        "memory.discard_held",
        "memory.citations_to_confirm",
        "memory.confirm_citation",
        "artefacts.get",
        "projects.get",
        "workspaces.working_copy_items",
        "workspaces.read_item",
    ]);
}

// Scenario: Contract — crews.charter is in the catalogue (SVC-SCR-9)
#[test]
fn contract_crews_charter_is_a_read_command_of_scope_any() {
    assert_in_catalogue(&["crews.charter"]);
    let meta = command("crews.charter").expect("in the catalogue");
    assert_eq!(meta.module, "crews");
    assert_eq!(meta.effect, Effect::Read);
    assert!(!PERSON_ONLY_COMMANDS.contains(&"crews.charter"));
    let accepted = accept_local("crews.charter", json!({ "crew_charter_id": OTHER }));
    assert!(accepted.is_ok());
    let unknown = accept_local(
        "crews.charter",
        json!({ "crew_charter_id": OTHER, "force": true }),
    );
    assert!(unknown.is_err(), "unknown fields are refused");
}

// Scenario: Security — only the person keeps, discards or confirms (SVC-SCR-2)
#[test]
fn security_only_the_person_keeps_discards_or_confirms() {
    for name in [
        "memory.keep_held",
        "memory.discard_held",
        "memory.confirm_citation",
    ] {
        assert!(PERSON_ONLY_COMMANDS.contains(&name), "{name}");
    }
    for name in ["memory.held_writes", "memory.citations_to_confirm"] {
        assert!(!PERSON_ONLY_COMMANDS.contains(&name), "{name}");
    }
}

// Scenario: Error path — a held write kept with a malformed hash (SVC-SCR-2)
#[test]
fn error_path_a_held_write_kept_with_a_malformed_hash() {
    let refusal = refused(
        "memory.keep_held",
        json!({"workspace_id": WS, "id": "h1", "content_hash": "ABC"}),
    );
    assert_eq!(refusal.code, "SVC_INPUT_INVALID");
    assert_eq!(refusal.at.as_deref(), Some("/input/content_hash"));
    assert!(accept_local(
        "memory.keep_held",
        json!({"workspace_id": WS, "id": "h1", "content_hash": "a".repeat(64)}),
    )
    .is_ok());
}

// Scenario: Error path — an origin path with a control character (SVC-SCR-5)
#[test]
fn error_path_an_origin_path_with_a_control_character() {
    let link = SourceLinkId::new();
    assert!(accept_local(
        "workspaces.read_item",
        json!({"source_link_id": link, "origin_path": "PROJ-12/spec.docx"}),
    )
    .is_ok());
    for bad in ["", "a\u{1}b"] {
        let refusal = refused(
            "workspaces.read_item",
            json!({"source_link_id": link, "origin_path": bad}),
        );
        assert_eq!(refusal.at.as_deref(), Some("/input/origin_path"), "{bad:?}");
    }
}

// Scenario: Contract — the generated update state carries the whole-computer case (SVC-SCR-6)
#[test]
fn contract_the_update_state_carries_the_whole_computer_case() {
    let state = serde_json::to_value(commands::setup::UpdateState::AdministratorNeeded).unwrap();
    assert_eq!(state, json!("administrator_needed"));
    let account = commands::knowledge_agents::ConnectionAccount {
        system: ordin_types::wire::knowledge_query::SourceSystem::Github,
        state: commands::knowledge_agents::AccountLinkState::NotSignedIn,
    };
    assert_eq!(
        serde_json::to_value(account).unwrap(),
        json!({"system": "github", "state": "not_signed_in"})
    );
}

// Scenario: Edge case — an artefact id no row has (SVC-SCR-3)
#[test]
fn edge_case_an_artefact_id_no_row_has() {
    let none = commands::artefacts::ArtefactsGetResult { artefact: None };
    assert_eq!(serde_json::to_value(none).unwrap(), json!({}));
}

struct NoConfluenceLink(ConfluenceLinkId);

#[async_trait::async_trait]
impl RefResolver for NoConfluenceLink {
    async fn content(&self, _: WorkspaceId, _: &ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _: WorkspaceId, _: EmailLinkId, _: &str) -> bool {
        true
    }
    async fn entity(&self, _: WorkspaceId, entity: EntityRef) -> bool {
        entity != EntityRef::ConfluenceLink(self.0)
    }
    async fn run_agent_name(&self, _: RunId) -> Option<String> {
        None
    }
    async fn plan_state(&self, _: RunId) -> PlanState {
        PlanState::default()
    }
    fn approval_verbs(&self) -> &[String] {
        &[]
    }
}

// Scenario: A checklist names a linked folder of its workspace (2C wire §13)
#[tokio::test]
async fn a_checklist_names_a_linked_folder_of_its_workspace() {
    let ctx = BlockCheckContext {
        workspace_id: WorkspaceId::new(),
        run_id: None,
        now: Timestamp::now(),
    };
    let plain =
        json!({"kind": "result", "of": "checklist", "title": "Ready to publish", "items": []});
    assert!(
        check_block(&plain, &ctx, &NoConfluenceLink(ConfluenceLinkId::new()))
            .await
            .is_ok()
    );
    let gone = ConfluenceLinkId::new();
    let raw = json!({"kind": "result", "of": "checklist", "title": "Ready to publish", "items": [], "confluence_link_id": gone});
    let rejections = check_block(&raw, &ctx, &NoConfluenceLink(gone))
        .await
        .unwrap_err();
    assert_eq!(rejections[0].code, "BLOCK_REF_UNRESOLVED");
    assert_eq!(rejections[0].at, "/confluence_link_id");
}

// ---- 2026-01-28: the memory models download (SVC-MD-1 to SVC-MD-3) ----

const MODELS_COMMANDS: [&str; 4] = [
    "app.memory_models",
    "app.download_memory_models",
    "app.stop_memory_models_download",
    "app.decline_memory_models",
];

// Scenario: Happy path — the catalogue carries the four rows
// spec: ordin-desktop/components/ServiceApi#SVC-MD-1
#[test]
fn happy_path_the_catalogue_carries_the_four_memory_models_rows() {
    assert_in_catalogue(&MODELS_COMMANDS);
    for name in MODELS_COMMANDS {
        let meta = command(name).expect("the row");
        assert_eq!(meta.scope, Scope::Local, "{name}");
    }
    assert_eq!(
        command("app.memory_models").expect("row").effect,
        Effect::Read
    );
    assert_eq!(
        command("app.download_memory_models").expect("row").effect,
        Effect::Change
    );
}

// Scenario: Happy path — the status answers the view (the view's wire form)
// spec: ordin-desktop/components/ServiceApi#SVC-MD-1
#[test]
fn happy_path_the_memory_models_view_has_its_wire_form() {
    let missing = commands::MemoryModelsView {
        state: commands::MemoryModelsState::Missing,
        size_bytes: 173_982_577,
        host: "github.com".to_owned(),
        received_bytes: 0,
        failure: None,
        ask: true,
    };
    assert_eq!(
        serde_json::to_value(&missing).unwrap(),
        json!({"state": "missing", "size_bytes": 173982577, "host": "github.com", "received_bytes": 0, "ask": true})
    );
    let failed = commands::MemoryModelsView {
        state: commands::MemoryModelsState::Failed,
        failure: Some("DSK_MODELS_OFFLINE".to_owned()),
        ..missing
    };
    assert_eq!(
        serde_json::to_value(&failed).unwrap()["failure"],
        "DSK_MODELS_OFFLINE"
    );
    assert_eq!(
        serde_json::to_value(commands::MemoryModelsState::NotInBuild).unwrap(),
        "not_in_build"
    );
}

// SVC-MD-3: an input other than {} is refused.
#[test]
fn error_path_a_memory_models_command_with_an_input_is_refused() {
    for name in MODELS_COMMANDS {
        assert!(accept_local(name, json!({})).is_ok(), "{name}");
        assert_eq!(
            refused(name, json!({"force": true})).code,
            "SVC_INPUT_INVALID",
            "{name}"
        );
    }
}

// Scenario: Edge case — a paired desktop cannot start the download
// spec: ordin-desktop/components/ServiceApi#SVC-MD-1
#[test]
fn edge_case_a_paired_desktop_cannot_start_the_download() {
    let body = envelope("app.download_memory_models", json!({}));
    assert_eq!(
        accept_with(&body, "app.download_memory_models", paired())
            .unwrap_err()
            .code,
        "SVC_COMMAND_LOCAL_ONLY"
    );
}

// Scenario: Happy path — progress reaches the screens as events (validation received ≤ size)
// spec: ordin-desktop/components/ServiceApi#SVC-MD-2
#[test]
fn happy_path_memory_models_changed_is_checked_and_decoded() {
    let view = json!({"state": "downloading", "size_bytes": 100, "host": "github.com", "received_bytes": 40, "ask": false});
    let event = ServiceEvent::from_journal("memory_models_changed", json!({ "models": view }))
        .expect("decoded");
    assert_eq!(event.kind(), EventKind::MemoryModelsChanged);
    let over = json!({"state": "downloading", "size_bytes": 100, "host": "github.com", "received_bytes": 101, "ask": false});
    assert_eq!(
        ServiceEvent::from_journal("memory_models_changed", json!({ "models": over }))
            .unwrap_err()
            .code,
        "SVC_EVENT_INVALID"
    );
}

// Scenario: Happy path — the working state reaches the screens (SVC-WK-1)
// spec: ordin-desktop/components/ServiceApi#SVC-WK-1
#[test]
fn happy_path_ordin_turn_changed_is_checked_and_decoded() {
    let w = "0192a1c5-0000-7000-8000-000000000001";
    let at = "2026-01-31T12:00:00.000Z";
    let event = ServiceEvent::from_journal(
        "ordin_turn_changed",
        json!({ "workspace_id": w, "state": "working", "doing": "searching the library", "since": at }),
    )
    .expect("decoded");
    assert_eq!(event.kind(), EventKind::OrdinTurnChanged);
    let ServiceEvent::OrdinTurnChanged { state, doing, .. } = &event else {
        panic!("ordin_turn_changed");
    };
    assert_eq!(*state, ordin_service_api::OrdinTurnState::Working);
    assert_eq!(doing.as_deref(), Some("searching the library"));
    // Without `doing`, and idle.
    let idle = ServiceEvent::from_journal(
        "ordin_turn_changed",
        json!({ "workspace_id": w, "state": "idle", "since": at }),
    )
    .expect("decoded");
    assert!(!idle
        .to_journal_data()
        .as_object()
        .unwrap()
        .contains_key("doing"));
    // `doing` is 1–80 characters.
    for doing in [String::new(), "x".repeat(81)] {
        assert_eq!(
            ServiceEvent::from_journal(
                "ordin_turn_changed",
                json!({ "workspace_id": w, "state": "working", "doing": doing, "since": at }),
            )
            .unwrap_err()
            .code,
            "SVC_EVENT_INVALID"
        );
    }
}

// Scenario: Contract — runs.try_again starts a stopped run's task again (SVC-TA-1)
// spec: ordin-desktop/components/ServiceApi#SVC-TA-1
#[test]
fn contract_runs_try_again_is_a_change_command_of_scope_any() {
    assert_in_catalogue(&["runs.try_again"]);
    let meta = command("runs.try_again").expect("in the catalogue");
    assert_eq!(meta.module, "runs");
    assert_eq!(meta.effect, Effect::Change);
    assert_eq!(meta.budget, Budget::Interactive);
    let run = "0192a0f1-3333-7444-8555-666677778888";
    let accepted = accept_local(
        "runs.try_again",
        json!({ "workspace_id": WS, "run_id": run }),
    );
    assert!(accepted.is_ok());
    let unknown = accept_local(
        "runs.try_again",
        json!({ "workspace_id": WS, "run_id": run, "task": "again" }),
    );
    assert!(unknown.is_err(), "unknown fields are refused");
}
