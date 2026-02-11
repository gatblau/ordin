//! OrdinHost.md amendment January 2026 — a task started from its form (OH-TF-1 to OH-TF-4; ADR-119):
//! one test per Gherkin scenario, named after it, and one per Error Table row.

use ordin_package_inspector::{TaskInput, TaskInputKind};
use ordin_service_api::blocks::{Block, FormBlock, FormField, FormInput, TaskFormRef};
use ordin_service_api::commands::ordin::{BlockAnswer, MessageAuthor};
use ordin_service_api::commands::runs::{
    RunsStartTaskInput, TaskInputValue, TaskProblem, TaskValue,
};
use ordin_types::domain::places::FolderAccess;
use ordin_types::wire::ContentRef;
use ordin_types::{AgentInstanceId, BlockId, PlaceId};
use ordin_usage_planner::WorkKind;
use serde_json::json;

use super::support::*;
use crate::{OhError, StartTaskOutcome};

const SUMMARISE: &str = "urn:ordin:default:commands:documents:summarise-document";
const COMPARE: &str = "urn:ordin:default:commands:documents:compare-documents";
const DRAFT: &str = "urn:ordin:default:commands:documents:draft-document";
const APPLY: &str = "urn:ordin:default:commands:lifecycle:apply";

fn input(name: &str, label: &str, kind: TaskInputKind) -> TaskInput {
    TaskInput {
        name: name.to_owned(),
        label: label.to_owned(),
        kind,
        required: true,
        hint: None,
        choices: Vec::new(),
        max: None,
        default: None,
        whole: false,
    }
}

fn optional(mut i: TaskInput) -> TaskInput {
    i.required = false;
    i
}

fn summarise_command() -> DirectoryCommand {
    let mut length = input("length", "Length", TaskInputKind::Choice);
    length.choices = vec![
        "A few lines".into(),
        "One page".into(),
        "Two to three pages".into(),
    ];
    length.default = Some("One page".into());
    DirectoryCommand {
        name: "/summarise-document".into(),
        command_id: SUMMARISE.into(),
        title: Some("Summarise a document".into()),
        inputs: Some(vec![
            input("document", "Document to summarise", TaskInputKind::Document),
            optional(input("reader", "Who it is for", TaskInputKind::ShortText)),
            length,
            optional(input(
                "focus",
                "Anything to bring out",
                TaskInputKind::LongText,
            )),
        ]),
        ..Default::default()
    }
}

fn compare_command() -> DirectoryCommand {
    DirectoryCommand {
        name: "/compare-documents".into(),
        command_id: COMPARE.into(),
        title: Some("Compare documents".into()),
        inputs: Some(vec![
            input(
                "reference",
                "Earlier or reference document",
                TaskInputKind::Document,
            ),
            input(
                "other",
                "Document to compare with it",
                TaskInputKind::Document,
            ),
            optional(input("focus", "What matters most", TaskInputKind::LongText)),
        ]),
        ..Default::default()
    }
}

fn draft_command() -> DirectoryCommand {
    let mut sources = optional(input(
        "sources",
        "Documents to build on",
        TaskInputKind::Documents,
    ));
    sources.max = Some(10);
    DirectoryCommand {
        name: "/draft-document".into(),
        command_id: DRAFT.into(),
        title: Some("Draft a document".into()),
        inputs: Some(vec![
            input(
                "request",
                "What the document should be",
                TaskInputKind::LongText,
            ),
            sources,
            optional(input(
                "example",
                "Template or example to follow",
                TaskInputKind::Document,
            )),
        ]),
        ..Default::default()
    }
}

fn apply_command() -> DirectoryCommand {
    DirectoryCommand {
        name: "/apply".into(),
        command_id: APPLY.into(),
        title: Some("Apply a plan phase".into()),
        inputs: Some(vec![
            input("plan", "Plan", TaskInputKind::Document),
            TaskInput {
                whole: true,
                ..input("phase", "Phase number", TaskInputKind::Number)
            },
        ]),
        ..Default::default()
    }
}

struct World {
    f: Fixture,
    agent: DirectoryAgent,
    docs: PlaceId,
}

/// The document agent in W with the four commands, the linked folder "Docs" on which it holds no grant.
async fn world(profile: &str, commands: Vec<DirectoryCommand>) -> World {
    let f = fixture().await;
    let mut agent = agent("Document agent");
    agent.commands = commands;
    *f.inspector.agents.lock().unwrap() = vec![agent.clone()];
    let docs = PlaceId::new();
    *f.workspaces.folders.lock().unwrap() = vec![AgentFolder {
        place_id: docs,
        name: "Docs".into(),
        read_only: false,
        granted: false,
    }];
    f.catalogue.identities.lock().unwrap().insert(
        agent.agent_instance_id,
        (
            ordin_types::wire::workflow_file::AgentRepositoryRef::ProfileKey("bundled".to_owned()),
            profile.to_owned(),
            "0.2.0".to_owned(),
            "Document agent".to_owned(),
        ),
    );
    World { f, agent, docs }
}

async fn documents_world() -> World {
    world(
        "urn:ordin:default:workspaces:document-agent",
        vec![summarise_command(), compare_command(), draft_command()],
    )
    .await
}

fn doc(place_id: PlaceId, path: &str) -> ContentRef {
    ContentRef::PlaceFile {
        place_id,
        relative_path: path.to_owned(),
    }
}

fn value(name: &str, value: TaskValue) -> TaskInputValue {
    TaskInputValue {
        name: name.to_owned(),
        value,
    }
}

fn text(name: &str, text: &str) -> TaskInputValue {
    value(name, TaskValue::Text { text: text.into() })
}

fn choice(name: &str, choice: &str) -> TaskInputValue {
    value(
        name,
        TaskValue::Choice {
            choice: choice.into(),
        },
    )
}

fn document(name: &str, place_id: PlaceId, path: &str) -> TaskInputValue {
    value(
        name,
        TaskValue::Document {
            document: doc(place_id, path),
        },
    )
}

impl World {
    fn request(&self, command_id: &str, values: Vec<TaskInputValue>) -> RunsStartTaskInput {
        RunsStartTaskInput {
            workspace_id: self.f.ws,
            agent_instance_id: self.agent.agent_instance_id,
            command_id: command_id.to_owned(),
            values,
            block_id: None,
        }
    }

    async fn start(&self, request: RunsStartTaskInput) -> Result<StartTaskOutcome, OhError> {
        self.f.host.start_task_from_form(request).await
    }

    fn tasks(&self) -> Vec<SubmittedTask> {
        self.f.supervisor.tasks.lock().unwrap().clone()
    }

    fn grants(&self) -> Vec<(AgentInstanceId, PlaceId, Option<FolderAccess>)> {
        self.f.workspaces.grants_set.lock().unwrap().clone()
    }

    fn holds_grant(&self) -> bool {
        self.f
            .workspaces
            .folders
            .lock()
            .unwrap()
            .iter()
            .any(|f| f.place_id == self.docs && f.granted)
    }

    fn problems(outcome: &StartTaskOutcome) -> Vec<(String, TaskProblem, Option<String>)> {
        match outcome {
            StartTaskOutcome::Refused { problems } => problems
                .iter()
                .map(|p| (p.name.clone(), p.problem, p.document.clone()))
                .collect(),
            other => panic!("refused, not {other:?}"),
        }
    }

    /// A waiting task form Ordin opened for `command`, as OH-TF-3 step 4 shows it.
    async fn ordin_form(&self, command: &DirectoryCommand, fields: Vec<FormField>) -> BlockId {
        self.f.open().await;
        let block = Block::Form(FormBlock {
            title: command.title.clone().unwrap_or_default(),
            verb: command.title.clone().unwrap_or_default(),
            target: ordin_types::wire::run::RunTarget::AgentInstance {
                agent_instance_id: self.agent.agent_instance_id,
            },
            command: Some(command.name.clone()),
            fields,
            task: Some(TaskFormRef {
                agent_instance_id: self.agent.agent_instance_id,
                command_id: command.command_id.clone(),
            }),
        });
        self.f
            .host
            .show_block(self.f.ws, MessageAuthor::Ordin, block, true)
            .await
            .expect("the form is shown")
    }
}

fn argument_pairs(task: &SubmittedTask) -> Vec<(String, String)> {
    task.0
        .arguments
        .iter()
        .map(|a| (a.name.clone(), a.value.clone()))
        .collect()
}

// Scenario: Happy path — the document agent summarises a chosen document (OH-TF-1)
// spec: ordin-desktop/components/OrdinHost#OH-TF-1
#[tokio::test]
async fn happy_path_the_document_agent_summarises_a_chosen_document() {
    let w = documents_world().await;
    let outcome = w
        .start(w.request(
            SUMMARISE,
            vec![
                document("document", w.docs, "plans/strategy.pdf"),
                choice("length", "One page"),
            ],
        ))
        .await
        .unwrap();
    let StartTaskOutcome::Started { run_id } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(
        w.grants(),
        vec![(w.agent.agent_instance_id, w.docs, Some(FolderAccess::Read))]
    );
    assert!(w.holds_grant());
    let tasks = w.tasks();
    assert_eq!(tasks.len(), 1);
    // OH-DLG-3: the run's command is the bare stem the Runtime Adapter accepts.
    assert_eq!(tasks[0].0.command.as_deref(), Some("summarise-document"));
    assert_eq!(
        argument_pairs(&tasks[0]),
        vec![
            (
                "document".to_owned(),
                "strategy.pdf (in Docs: plans/strategy.pdf)".to_owned()
            ),
            ("length".to_owned(), "One page".to_owned()),
        ]
    );
    assert_eq!(tasks[0].0.attached_artefacts.len(), 1);
    assert!(tasks[0].2, "started by the person");
    let recorded = w.f.journal.artefacts.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].path, "plans/strategy.pdf");
    assert_eq!(recorded[0].producing_run_id, None);
    assert_eq!(w.f.supervisor.task_runs.lock().unwrap()[0], run_id);
    // Nothing is written to the conversation and nothing is sent to Ordin.
    assert_eq!(w.f.count("SELECT count(*) FROM ordin_messages").await, 0);
    assert!(w.f.supervisor.turns.lock().unwrap().is_empty());
}

// Scenario: Edge — the default fills an empty choice (OH-TF-1)
#[tokio::test]
async fn edge_the_default_fills_an_empty_choice() {
    let w = documents_world().await;
    w.start(w.request(
        SUMMARISE,
        vec![document("document", w.docs, "plans/strategy.pdf")],
    ))
    .await
    .unwrap();
    let pairs = argument_pairs(&w.tasks()[0]);
    assert!(pairs.contains(&("length".to_owned(), "One page".to_owned())));
    assert!(
        !pairs.iter().any(|(n, _)| n == "reader"),
        "an empty input is not listed"
    );
}

// Scenario: Error — a document that left the workspace (OH-TF-1)
#[tokio::test]
async fn error_a_document_that_left_the_workspace() {
    let w = documents_world().await;
    w.f.files
        .missing
        .lock()
        .unwrap()
        .push("plans/strategy.pdf".into());
    let outcome = w
        .start(w.request(
            SUMMARISE,
            vec![document("document", w.docs, "plans/strategy.pdf")],
        ))
        .await
        .unwrap();
    assert_eq!(
        World::problems(&outcome),
        vec![(
            "document".to_owned(),
            TaskProblem::DocumentGone,
            Some("strategy.pdf".to_owned())
        )]
    );
    assert!(w.grants().is_empty(), "no grant is made");
    assert!(w.tasks().is_empty(), "no task starts");
}

// A document in a place that is not the workspace's is gone too (OH-TF-1 step 3).
#[tokio::test]
async fn error_a_document_in_another_place_is_gone() {
    let w = documents_world().await;
    let outcome = w
        .start(w.request(
            SUMMARISE,
            vec![document("document", PlaceId::new(), "x/q2.pdf")],
        ))
        .await
        .unwrap();
    assert_eq!(
        World::problems(&outcome),
        vec![(
            "document".to_owned(),
            TaskProblem::DocumentGone,
            Some("q2.pdf".to_owned())
        )]
    );
}

// Scenario Outline: Error — values that break the inputs (OH-TF-1)
#[tokio::test]
async fn error_values_that_break_the_inputs() {
    let w = world(
        "urn:ordin:default:workspaces:document-agent",
        vec![apply_command()],
    )
    .await;
    let plan = || document("plan", w.docs, "plans/03-key-rotation.md");
    let cases: Vec<(Vec<TaskInputValue>, TaskProblem, &str)> = vec![
        (vec![text("phase", "2")], TaskProblem::RequiredEmpty, "plan"),
        (
            vec![plan(), text("phase", "two")],
            TaskProblem::NotANumber,
            "phase",
        ),
        // A whole-number input refuses a fraction (AgentModel.md AM-TF-2, amended January 2026).
        (
            vec![plan(), text("phase", "2.5")],
            TaskProblem::NotANumber,
            "phase",
        ),
        (
            vec![plan(), text("phase", "2"), text("step", "1")],
            TaskProblem::UnknownInput,
            "step",
        ),
    ];
    for (values, expected, name) in cases {
        let outcome = w.start(w.request(APPLY, values)).await.unwrap();
        let problems = World::problems(&outcome);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].0, name);
        assert_eq!(problems[0].1, expected);
    }
    assert!(w.tasks().is_empty());
}

// The rest of step 3's rules: too long, a date, a choice and a value of the wrong kind.
#[tokio::test]
async fn error_the_other_value_rules() {
    let mut due = input("due", "Due", TaskInputKind::Date);
    due.required = false;
    let mut tone = optional(input("tone", "Tone", TaskInputKind::Choice));
    tone.choices = vec!["Everyday".into(), "Formal".into()];
    let command = DirectoryCommand {
        name: "/note".into(),
        command_id: "urn:ordin:default:commands:documents:note".into(),
        title: Some("Write a note".into()),
        inputs: Some(vec![
            optional(input("subject", "Subject", TaskInputKind::ShortText)),
            due,
            tone,
            optional(input("source", "Source", TaskInputKind::Document)),
        ]),
        ..Default::default()
    };
    let w = world("urn:ordin:default:workspaces:document-agent", vec![command]).await;
    let id = "urn:ordin:default:commands:documents:note";
    let outcome = w
        .start(w.request(
            id,
            vec![
                text("subject", &"s".repeat(201)),
                text("due", "2026-02-30"),
                choice("tone", "Casual"),
                text("source", "notes.md"),
            ],
        ))
        .await
        .unwrap();
    let problems: Vec<TaskProblem> = World::problems(&outcome).iter().map(|p| p.1).collect();
    assert_eq!(
        problems,
        vec![
            TaskProblem::TooLong,
            TaskProblem::NotADate,
            TaskProblem::NotAChoice,
            TaskProblem::WrongKind
        ]
    );
    let outcome = w
        .start(w.request(id, vec![text("subject", "one\ntwo")]))
        .await
        .unwrap();
    assert_eq!(World::problems(&outcome)[0].1, TaskProblem::TooLong);
}

// Scenario: Error — a documents field with one document (OH-TF-1)
#[tokio::test]
async fn error_a_documents_field_with_one_document() {
    let w = documents_world().await;
    let outcome = w
        .start(w.request(
            DRAFT,
            vec![
                text("request", "A one-page brief for the board."),
                value(
                    "sources",
                    TaskValue::Documents {
                        documents: vec![doc(w.docs, "a.md")],
                    },
                ),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(
        World::problems(&outcome),
        vec![("sources".to_owned(), TaskProblem::TooFewDocuments, None)]
    );
    let many: Vec<ContentRef> = (0..11).map(|i| doc(w.docs, &format!("{i}.md"))).collect();
    let outcome = w
        .start(w.request(
            DRAFT,
            vec![
                text("request", "A brief."),
                value("sources", TaskValue::Documents { documents: many }),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(
        World::problems(&outcome)[0].1,
        TaskProblem::TooManyDocuments
    );
}

// Scenario: Edge — the form Ordin opened is sent (OH-TF-1, OH-TF-3)
#[tokio::test]
async fn edge_the_form_ordin_opened_is_sent() {
    let w = documents_world().await;
    let block_id = w
        .ordin_form(
            &compare_command(),
            vec![FormField {
                name: "reference".into(),
                label: "Earlier or reference document".into(),
                required: true,
                input: FormInput::Document {
                    value: Some(doc(w.docs, "q2.pdf")),
                },
                filled_by_ordin: true,
            }],
        )
        .await;
    let turns_before = w.f.supervisor.turns.lock().unwrap().len();
    let mut request = w.request(
        COMPARE,
        vec![
            document("reference", w.docs, "q2.pdf"),
            document("other", w.docs, "q3.pdf"),
        ],
    );
    request.block_id = Some(block_id);
    let outcome = w.start(request).await.unwrap();
    assert!(
        matches!(outcome, StartTaskOutcome::Started { .. }),
        "{outcome:?}"
    );
    assert!(
        waiting_of(&w.f, "form").is_empty(),
        "the form's card stops waiting"
    );
    assert_eq!(
        w.f.count("SELECT count(*) FROM ordin_messages WHERE text = 'Form sent'")
            .await,
        1
    );
    assert_eq!(
        w.f.supervisor.turns.lock().unwrap().len(),
        turns_before,
        "no turn is sent to Ordin"
    );
    // A second press finds the form answered.
    let mut again = w.request(
        COMPARE,
        vec![
            document("reference", w.docs, "q2.pdf"),
            document("other", w.docs, "q3.pdf"),
        ],
    );
    again.block_id = Some(block_id);
    assert_eq!(
        w.start(again).await.unwrap_err().code(),
        "OH_BLOCK_ANSWERED"
    );
}

fn waiting_of(f: &Fixture, kind: &str) -> Vec<crate::types::WaitingBlock> {
    f.host
        .waiting_blocks()
        .into_iter()
        .filter(|b| b.workspace_id == f.ws && b.kind == kind)
        .collect()
}

// Scenario: Happy path — Ordin starts a complete request without a form (OH-TF-3)
// spec: ordin-desktop/components/OrdinHost#OH-TF-3
#[tokio::test]
async fn happy_path_ordin_starts_a_complete_request_without_a_form() {
    let w = documents_world().await;
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    w.f.open().await;
    // OH-TF-5: the person names the task, so its inputs are honoured.
    w.f.say(
        "Run Summarise a document on strategy.pdf, a few lines",
        vec![],
    )
    .await;
    let out =
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Summarise the strategy",
                "command": "/summarise-document",
                "inputs": { "document": "Docs: plans/strategy.pdf", "length": "A few lines" }
            }),
        )
        .await
        .unwrap();
    assert_eq!(out.text, "Started: Document agent is working on it.");
    assert!(w.grants().is_empty(), "Ordin never grants access by itself");
    let tasks = w.tasks();
    assert_eq!(tasks.len(), 1);
    assert!(!tasks[0].2, "Ordin started it");
    assert!(argument_pairs(&tasks[0]).contains(&("length".to_owned(), "A few lines".to_owned())));
}

// OH-TF-3 step 4: a request that is not complete opens the task's form, filled with what Ordin gave.
#[tokio::test]
async fn edge_an_incomplete_request_opens_the_form() {
    let w = documents_world().await;
    w.f.open().await;
    w.f.say("Please run Compare documents on the Q2 quote", vec![])
        .await;
    let out =
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Compare the quotes",
                "command": "/compare-documents",
                "inputs": { "reference": "Docs: q2.pdf", "colour": "red" }
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        out.text,
        "I opened the form for Compare documents beside the conversation; the person checks it and starts it."
    );
    assert!(w.tasks().is_empty());
    let forms = waiting_of(&w.f, "form");
    assert_eq!(forms.len(), 1);
    let row = {
        let mut c = w.f.pool.acquire().await.unwrap();
        crate::store::block(&mut c, forms[0].block_id)
            .await
            .unwrap()
            .unwrap()
    };
    let Block::Form(form) = row.block else {
        panic!("a form")
    };
    assert_eq!(form.verb, "Compare documents");
    assert_eq!(
        form.task.as_ref().map(|t| t.command_id.as_str()),
        Some(COMPARE)
    );
    assert_eq!(form.fields.len(), 3);
    assert!(form.fields[0].filled_by_ordin);
    assert!(!form.fields[1].filled_by_ordin);
}

// Error-table row: `delegate` with `arguments` for a task with inputs (OH-TF-3).
#[tokio::test]
async fn error_delegate_with_arguments_for_a_task_with_inputs() {
    let w = documents_world().await;
    w.f.open().await;
    w.f.say("/summarise-document strategy.pdf", vec![]).await;
    let e =
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Summarise it",
                "command": "/summarise-document",
                "arguments": "strategy.pdf"
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_INPUT_INVALID");
    assert_eq!(
        e.message,
        "Summarise a document takes named inputs; pass them as inputs, not arguments."
    );
}

// Scenario: Error — a task form answered as a chat answer (OH-TF-2)
// spec: ordin-desktop/components/OrdinHost#OH-TF-2
#[tokio::test]
async fn error_a_task_form_answered_as_a_chat_answer() {
    let w = documents_world().await;
    let block_id = w.ordin_form(&compare_command(), vec![]).await;
    let turns_before = w.f.supervisor.turns.lock().unwrap().len();
    let e =
        w.f.host
            .answer_block(
                w.f.ws,
                block_id,
                BlockAnswer::Form {
                    fields: Default::default(),
                },
            )
            .await
            .unwrap_err();
    assert_eq!(e.code(), "OH_INPUT_INVALID");
    assert_eq!(w.f.supervisor.turns.lock().unwrap().len(), turns_before);
    assert_eq!(waiting_of(&w.f, "form").len(), 1, "the form still waits");
}

/// A pack question for the document agent.
fn pack(agent: AgentInstanceId) -> ordin_package_catalogue::PendingChoice {
    ordin_package_catalogue::PendingChoice {
        agent_instance_id: agent,
        rule_index: 0,
        at_least: 1,
        class_label: "style".to_owned(),
        choices: vec![ordin_package_catalogue::PendingChoiceOption {
            pack: "urn:pack:plain".to_owned(),
            label: "Plain".to_owned(),
        }],
    }
}

// Scenario: Edge — a question before the start grants nothing (OH-TF-1)
#[tokio::test]
async fn edge_a_question_before_the_start_grants_nothing() {
    let w = documents_world().await;
    w.f.open().await;
    *w.f.catalogue.pending.lock().unwrap() = vec![pack(w.agent.agent_instance_id)];
    let outcome = w
        .start(w.request(
            SUMMARISE,
            vec![document("document", w.docs, "plans/strategy.pdf")],
        ))
        .await
        .unwrap();
    assert!(
        matches!(outcome, StartTaskOutcome::Waiting { .. }),
        "{outcome:?}"
    );
    assert!(!w.holds_grant());
    assert!(w.tasks().is_empty());
    let asked = waiting_of(&w.f, "question");
    assert_eq!(asked.len(), 1);
    w.f.catalogue.pending.lock().unwrap().clear();
    w.f.host
        .answer_block(
            w.f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "Use Plain".to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(w.holds_grant(), "the grant is made at the start");
    assert_eq!(w.tasks().len(), 1);
}

// Scenario: Edge — a refused start removes the grant it made (OH-TF-1)
#[tokio::test]
async fn edge_a_refused_start_removes_the_grant_it_made() {
    let w = documents_world().await;
    w.f.f
        .always("supervisor.submit_task", "SUP_QUEUE_FULL", true);
    let e = w
        .start(w.request(
            SUMMARISE,
            vec![document("document", w.docs, "plans/strategy.pdf")],
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(&e, OhError::DependencyFailed { code, .. } if code == "SUP_QUEUE_FULL"),
        "{e:?}"
    );
    assert_eq!(
        w.grants(),
        vec![
            (w.agent.agent_instance_id, w.docs, Some(FolderAccess::Read)),
            (w.agent.agent_instance_id, w.docs, None)
        ]
    );
    assert!(!w.holds_grant());
}

// Error-table row: a grant, a document read or an artefact record fails; grants made are removed.
#[tokio::test]
async fn error_a_failed_record_removes_the_grant_and_is_a_dependency_failure() {
    let w = documents_world().await;
    w.f.f
        .always("journal.record_artefact", "JRN_STORE_FAILED", true);
    let e = w
        .start(w.request(
            SUMMARISE,
            vec![document("document", w.docs, "plans/strategy.pdf")],
        ))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DEPENDENCY_FAILED");
    assert!(e.retryable());
    assert!(!w.holds_grant());
    assert!(w.tasks().is_empty());
    // A read that fails for another reason than a missing file.
    let w = documents_world().await;
    w.f.f.always("files.read_file", "FILE_IO", true);
    let e = w
        .start(w.request(
            SUMMARISE,
            vec![document("document", w.docs, "plans/strategy.pdf")],
        ))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DEPENDENCY_FAILED");
}

fn repo(name: &str, place_id: PlaceId) -> LinkedRepo {
    LinkedRepo {
        host: ordin_types::domain::ShareHost::Github,
        repository: name.to_owned(),
        host_name: "github.com".to_owned(),
        place_id,
    }
}

async fn engineering_world() -> World {
    world(
        crate::task_form::SOFTWARE_ENGINEERING_PROFILE,
        vec![apply_command()],
    )
    .await
}

// Scenario: Happy path — a software-engineering task runs in its own worktree (OH-TF-1)
#[tokio::test]
async fn happy_path_a_software_engineering_task_runs_in_its_own_worktree() {
    let w = engineering_world().await;
    *w.f.workspaces.repos.lock().unwrap() = vec![repo("acme/payments", w.docs)];
    w.f.projects
        .projects
        .lock()
        .unwrap()
        .insert(w.docs, (ordin_types::ProjectId::new(), true));
    let outcome = w
        .start(w.request(
            APPLY,
            vec![
                document("plan", w.docs, "plans/03-key-rotation.md"),
                text("phase", "2"),
            ],
        ))
        .await
        .unwrap();
    assert!(
        matches!(outcome, StartTaskOutcome::Started { .. }),
        "{outcome:?}"
    );
    {
        let coding = w.f.projects.coding.lock().unwrap();
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].request, "Apply a plan phase");
        // OH-DLG-3: the coding task runs the command under its bare stem too.
        assert_eq!(coding[0].task_name, "apply");
    }
    let tasks = w.tasks();
    assert_eq!(tasks[0].1, WorkKind::Code);
    assert!(tasks[0].3.is_some(), "a coding task");
}

// Error-table rows: a software-engineering task with no repository open as a project, or more than
// four and no chosen document in one; two to four ask which one.
#[tokio::test]
async fn error_a_software_engineering_task_needs_its_repository() {
    let w = engineering_world().await;
    let outcome = w
        .start(w.request(
            APPLY,
            vec![document("plan", w.docs, "plans/03.md"), text("phase", "2")],
        ))
        .await;
    assert_eq!(outcome.unwrap_err().code(), "OH_INPUT_INVALID");
    // Two repositories, the plan in neither: the conversation asks which.
    let w = engineering_world().await;
    w.f.open().await;
    let (a, b) = (PlaceId::new(), PlaceId::new());
    *w.f.workspaces.repos.lock().unwrap() = vec![repo("acme/payments", a), repo("acme/ledger", b)];
    for place in [a, b] {
        w.f.projects
            .projects
            .lock()
            .unwrap()
            .insert(place, (ordin_types::ProjectId::new(), true));
    }
    let outcome = w
        .start(w.request(
            APPLY,
            vec![document("plan", w.docs, "plans/03.md"), text("phase", "2")],
        ))
        .await
        .unwrap();
    assert!(
        matches!(outcome, StartTaskOutcome::Waiting { .. }),
        "{outcome:?}"
    );
    assert!(!w.holds_grant(), "nothing granted");
    assert!(w.tasks().is_empty(), "nothing started");
    let asked = waiting_of(&w.f, "question");
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].title,
        "Which repository should Document agent work in?"
    );
    w.f.host
        .answer_block(
            w.f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "acme/ledger".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        w.f.projects.coding.lock().unwrap().len(),
        1,
        "the start continues from (c)"
    );
    assert_eq!(w.tasks().len(), 1);
    // Five repositories and no chosen document in one.
    let w = engineering_world().await;
    let five: Vec<LinkedRepo> = (0..5)
        .map(|i| repo(&format!("acme/r{i}"), PlaceId::new()))
        .collect();
    for r in &five {
        w.f.projects
            .projects
            .lock()
            .unwrap()
            .insert(r.place_id, (ordin_types::ProjectId::new(), true));
    }
    *w.f.workspaces.repos.lock().unwrap() = five;
    let outcome = w
        .start(w.request(
            APPLY,
            vec![document("plan", w.docs, "plans/03.md"), text("phase", "2")],
        ))
        .await;
    assert_eq!(outcome.unwrap_err().code(), "OH_INPUT_INVALID");
}

// Scenario: Error — a task whose inputs do not read (OH-TF-1)
#[tokio::test]
async fn error_a_task_whose_inputs_do_not_read() {
    let command = DirectoryCommand {
        name: "/compare-quotes".into(),
        command_id: "urn:ordin:acme:commands:compare-quotes".into(),
        form_unreadable: true,
        ..Default::default()
    };
    let w = world("urn:ordin:acme:workspaces:analyst", vec![command]).await;
    let e = w
        .start(w.request("urn:ordin:acme:commands:compare-quotes", vec![]))
        .await
        .unwrap_err();
    assert!(matches!(e, OhError::TaskUnreadable));
    assert_eq!(e.code(), "OH_TASK_UNREADABLE");
    assert!(w.tasks().is_empty());
}

// Error-table rows: an agent not in the directory, a task it does not offer, a block that is no task
// form of this task, and a workspace that is not here.
#[tokio::test]
async fn error_an_unknown_agent_task_block_or_workspace() {
    let w = documents_world().await;
    let mut request = w.request(SUMMARISE, vec![]);
    request.agent_instance_id = AgentInstanceId::new();
    let e = w.start(request).await.unwrap_err();
    assert_eq!(e.code(), "OH_AGENT_UNKNOWN");
    assert_eq!(e.message(), "That agent isn't in this workspace any more.");
    let e = w
        .start(w.request("urn:ordin:default:commands:documents:gone", vec![]))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_COMMAND_UNKNOWN");
    assert_eq!(e.message(), "This agent no longer offers that task.");
    let block_id = w.ordin_form(&compare_command(), vec![]).await;
    let mut request = w.request(
        SUMMARISE,
        vec![document("document", w.docs, "plans/strategy.pdf")],
    );
    request.block_id = Some(block_id);
    assert_eq!(
        w.start(request).await.unwrap_err().code(),
        "OH_BLOCK_UNKNOWN"
    );
    w.f.f
        .always("workspaces.ensure_active", "WS_NOT_FOUND", false);
    assert_eq!(
        w.start(w.request(SUMMARISE, vec![]))
            .await
            .unwrap_err()
            .code(),
        "OH_WORKSPACE_UNKNOWN"
    );
}

// A task without inputs takes its one value `arguments`, filled into `$ARGUMENTS` (OH-TF-1 step 6).
#[tokio::test]
async fn edge_a_task_without_inputs_takes_its_one_value() {
    let command = DirectoryCommand {
        name: "/rca".into(),
        command_id: "urn:ordin:default:commands:intent:rca".into(),
        argument_hint: Some("<error>".into()),
        ..Default::default()
    };
    let w = world("urn:ordin:acme:workspaces:analyst", vec![command]).await;
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    w.start(w.request(
        "urn:ordin:default:commands:intent:rca",
        vec![text("arguments", "panic: boom\r\nexit 2")],
    ))
    .await
    .unwrap();
    assert_eq!(
        argument_pairs(&w.tasks()[0]),
        vec![("arguments".to_owned(), "panic: boom\nexit 2".to_owned())]
    );
}

// OH-TF-4: the directory names each task's title and inputs.
// spec: ordin-desktop/components/OrdinHost#OH-TF-4
#[test]
fn edge_the_directory_names_each_tasks_title_and_inputs() {
    let line = crate::seed::task_line(&summarise_command());
    assert_eq!(
        line,
        " — \"Summarise a document\"; inputs: document (document), reader (short-text, optional), length (choice) [A few lines, One page, Two to three pages], focus (long-text, optional)"
    );
    assert_eq!(crate::seed::task_line(&DirectoryCommand::default()), "");
}

// ---------------------------------------------------------------- OH-TW-1, OH-TW-2: the results folder

/// The results folder's path in team workspace W, as the scenario names it.
fn results_path(w: &World) -> String {
    format!(
        "/Users/asha/Documents/ordin/Linked copies/results-{}",
        w.f.ws
    )
}

fn results_sentence(w: &World) -> String {
    format!("Save the files this task makes in {}.", results_path(w))
}

fn agent_grant(
    w: &World,
    agent: AgentInstanceId,
    place: PlaceId,
    access: FolderAccess,
) -> ordin_types::domain::places::FolderGrant {
    ordin_types::domain::places::FolderGrant {
        id: ordin_types::FolderGrantId::new(),
        workspace_id: w.f.ws,
        grantee: ordin_types::domain::places::Grantee::AgentInstance {
            agent_instance_id: agent,
        },
        place_id: place,
        access,
        granted_at: ordin_types::time::Timestamp::now(),
    }
}

/// W is a team workspace with a results folder; the document agent holds `access` on it (None: no
/// grant). Returns the folder's place.
fn team(w: &World, access: Option<FolderAccess>) -> PlaceId {
    let place = PlaceId::new();
    *w.f.workspaces.results_folder.lock().unwrap() = Some(place);
    w.f.files
        .places
        .lock()
        .unwrap()
        .insert(place, results_path(w));
    if let Some(access) = access {
        let grant = agent_grant(w, w.agent.agent_instance_id, place, access);
        w.f.workspaces.grants.lock().unwrap().push(grant);
    }
    place
}

fn compare_values(w: &World) -> Vec<TaskInputValue> {
    vec![
        document("reference", w.docs, "quotes/acme.pdf"),
        document("other", w.docs, "quotes/kestrel.pdf"),
    ]
}

fn results_arguments(task: &SubmittedTask) -> Vec<String> {
    task.0
        .arguments
        .iter()
        .filter(|a| a.name == "results_folder")
        .map(|a| a.value.clone())
        .collect()
}

// Scenario: Happy path — a task in a team workspace names the results folder (OH-TW-1)
// spec: ordin-desktop/components/OrdinHost#OH-TW-1
#[tokio::test]
async fn happy_path_a_task_in_a_team_workspace_names_the_results_folder() {
    let w = documents_world().await;
    team(&w, Some(FolderAccess::ReadAndChange));
    let outcome = w
        .start(w.request(COMPARE, compare_values(&w)))
        .await
        .unwrap();
    assert!(
        matches!(outcome, StartTaskOutcome::Started { .. }),
        "{outcome:?}"
    );
    let tasks = w.tasks();
    assert_eq!(tasks.len(), 1);
    let last = tasks[0].0.arguments.last().expect("arguments");
    assert_eq!(last.name, "results_folder");
    assert_eq!(
        last.value,
        format!(
            "Save the files this task makes in /Users/asha/Documents/ordin/Linked copies/results-{}.",
            w.f.ws
        )
    );
    // The inputs come first, in input order.
    assert_eq!(tasks[0].0.arguments[0].name, "reference");
    // No grant is made here (OH-TW-1 item 3): only the form's own read grant on Docs.
    assert_eq!(
        w.grants(),
        vec![(w.agent.agent_instance_id, w.docs, Some(FolderAccess::Read))]
    );
}

// Scenario: Edge — an agent without access to the results folder is told nothing (OH-TW-1)
#[tokio::test]
async fn edge_an_agent_without_access_to_the_results_folder_is_told_nothing() {
    let w = documents_world().await;
    team(&w, None);
    w.start(w.request(COMPARE, compare_values(&w)))
        .await
        .unwrap();
    assert!(results_arguments(&w.tasks()[0]).is_empty());
    assert_eq!(w.f.f.count("files.place"), 0, "the path is not even read");
}

// OH-TW-1: read alone is not enough; nor is another agent's grant.
#[tokio::test]
async fn edge_read_alone_or_another_agents_grant_names_nothing() {
    let w = documents_world().await;
    let place = team(&w, Some(FolderAccess::Read));
    let other = agent_grant(
        &w,
        AgentInstanceId::new(),
        place,
        FolderAccess::ReadAndChange,
    );
    w.f.workspaces.grants.lock().unwrap().push(other);
    w.start(w.request(COMPARE, compare_values(&w)))
        .await
        .unwrap();
    assert!(results_arguments(&w.tasks()[0]).is_empty());
}

// OH-TW-1: a workspace without a results folder (an own workspace) names nothing.
#[tokio::test]
async fn edge_a_workspace_without_a_results_folder_names_nothing() {
    let w = documents_world().await;
    w.start(w.request(COMPARE, compare_values(&w)))
        .await
        .unwrap();
    assert!(results_arguments(&w.tasks()[0]).is_empty());
    assert_eq!(w.f.f.count("workspaces.grants"), 0);
}

// OH-TW-1 item 1: a command delegated without inputs gains the argument after its other arguments.
#[tokio::test]
async fn edge_a_delegated_command_gains_the_argument_after_its_own() {
    let w = documents_world().await;
    team(&w, Some(FolderAccess::ReadAndChange));
    // The agent may use Docs, so no access card holds the task back (OH-ACC-1).
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    w.f.inspector.agents.lock().unwrap()[0]
        .commands
        .push(DirectoryCommand {
            name: "compare-quotes".into(),
            ..Default::default()
        });
    w.f.open().await;
    w.f.tool(
        "delegate",
        json!({
            "agent": w.agent.agent_instance_id.to_string(),
            "brief": "Compare the quotes",
            "command": "compare-quotes",
            "arguments": "Kestrel and Acme"
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        argument_pairs(&w.tasks()[0]),
        vec![
            ("arguments".to_owned(), "Kestrel and Acme".to_owned()),
            ("results_folder".to_owned(), results_sentence(&w)),
        ]
    );
}

// OH-TW-1 item 1: inputs that already name `results_folder` get no second one.
#[tokio::test]
async fn edge_inputs_that_already_name_results_folder_get_no_second_one() {
    let command = DirectoryCommand {
        name: "/file-results".into(),
        command_id: "urn:ordin:acme:commands:file-results".into(),
        title: Some("File the results".into()),
        inputs: Some(vec![input(
            "results_folder",
            "Where to file them",
            TaskInputKind::ShortText,
        )]),
        ..Default::default()
    };
    let w = world("urn:ordin:acme:workspaces:analyst", vec![command]).await;
    team(&w, Some(FolderAccess::ReadAndChange));
    // The agent may use Docs, so no access card holds the task back (OH-ACC-1).
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    w.start(w.request(
        "urn:ordin:acme:commands:file-results",
        vec![text("results_folder", "Archive")],
    ))
    .await
    .unwrap();
    assert_eq!(
        argument_pairs(&w.tasks()[0]),
        vec![("results_folder".to_owned(), "Archive".to_owned())]
    );
}

fn numbered_arguments(n: usize) -> Vec<ordin_types::domain::runs::TaskArgument> {
    (0..n)
        .map(|i| ordin_types::domain::runs::TaskArgument {
            name: format!("a{i}"),
            value: "x".into(),
        })
        .collect()
}

// OH-TW-1 item 1: a task that already holds 50 arguments gets no such argument.
#[tokio::test]
async fn edge_a_task_with_fifty_arguments_gets_no_results_folder() {
    let w = documents_world().await;
    team(&w, Some(FolderAccess::ReadAndChange));
    let mut full = numbered_arguments(50);
    let mut no_text = None;
    w.f.host
        .name_results_folder(w.f.ws, w.agent.agent_instance_id, &mut full, &mut no_text)
        .await;
    assert_eq!(full, numbered_arguments(50), "nothing is added");
    let mut room = numbered_arguments(49);
    w.f.host
        .name_results_folder(w.f.ws, w.agent.agent_instance_id, &mut room, &mut no_text)
        .await;
    assert_eq!(room.len(), 50);
    assert_eq!(room[49].name, "results_folder");
    assert_eq!(room[49].value, results_sentence(&w));
}

// OH-TW-1 item 1: a coding task gets no such argument; its files reach the team through the
// repository.
#[tokio::test]
async fn edge_a_coding_task_gets_no_results_folder() {
    let w = engineering_world().await;
    team(&w, Some(FolderAccess::ReadAndChange));
    *w.f.workspaces.repos.lock().unwrap() = vec![repo("acme/payments", w.docs)];
    w.f.projects
        .projects
        .lock()
        .unwrap()
        .insert(w.docs, (ordin_types::ProjectId::new(), true));
    w.start(w.request(
        APPLY,
        vec![
            document("plan", w.docs, "plans/03-key-rotation.md"),
            text("phase", "2"),
        ],
    ))
    .await
    .unwrap();
    let tasks = w.tasks();
    assert_eq!(tasks[0].1, WorkKind::Code);
    assert!(results_arguments(&tasks[0]).is_empty());
    assert_eq!(w.f.f.count("workspaces.results_folder"), 0);
}

// OH-TW-1 item 2: a free-text task gains the sentence after its text and one blank line.
#[tokio::test]
async fn edge_a_free_text_task_gains_the_sentence_after_a_blank_line() {
    let w = documents_world().await;
    team(&w, Some(FolderAccess::ReadAndChange));
    // The agent may use Docs, so no access card holds the task back (OH-ACC-1).
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    w.f.open().await;
    w.f.tool(
        "delegate",
        json!({ "agent": w.agent.agent_instance_id.to_string(), "brief": "Compare the quotes" }),
    )
    .await
    .unwrap();
    let tasks = w.tasks();
    let expected = format!("Compare the quotes\n\n{}", results_sentence(&w));
    assert_eq!(tasks[0].0.free_text.as_deref(), Some(expected.as_str()));
    assert!(tasks[0].0.arguments.is_empty());
}

// OH-TW-1 item 2: the text stays within 8,000 characters, or nothing is added.
#[tokio::test]
async fn edge_a_free_text_over_eight_thousand_characters_gains_nothing() {
    let w = documents_world().await;
    team(&w, Some(FolderAccess::ReadAndChange));
    // The agent may use Docs, so no access card holds the task back (OH-ACC-1).
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    let added = 2 + results_sentence(&w).chars().count();
    let mut no_arguments = Vec::new();
    // Exactly 8,000 characters with the sentence: added.
    let mut fits = Some("é".repeat(8_000 - added));
    w.f.host
        .name_results_folder(
            w.f.ws,
            w.agent.agent_instance_id,
            &mut no_arguments,
            &mut fits,
        )
        .await;
    let fits = fits.expect("text");
    assert_eq!(fits.chars().count(), 8_000);
    assert!(fits.ends_with(&results_sentence(&w)));
    // One character more: nothing is added.
    let long = "é".repeat(8_001 - added);
    let mut over = Some(long.clone());
    w.f.host
        .name_results_folder(
            w.f.ws,
            w.agent.agent_instance_id,
            &mut no_arguments,
            &mut over,
        )
        .await;
    assert_eq!(over, Some(long));
    assert!(no_arguments.is_empty());
    // Through `delegate`: a brief of 8,000 characters starts unchanged.
    w.f.open().await;
    let brief = "b".repeat(8_000);
    w.f.tool(
        "delegate",
        json!({ "agent": w.agent.agent_instance_id.to_string(), "brief": brief }),
    )
    .await
    .unwrap();
    assert_eq!(w.tasks()[0].0.free_text.as_deref(), Some(brief.as_str()));
}

// OH-TW-2: a WorkspaceError or FileError leaves the argument out, and the task starts as before.
// spec: ordin-desktop/components/OrdinHost#OH-TW-2
#[tokio::test]
async fn error_an_unread_results_folder_leaves_the_argument_out_and_the_task_starts() {
    for (call, code, retryable) in [
        (
            "workspaces.results_folder",
            "WS_DATASTORE_UNAVAILABLE",
            true,
        ),
        ("workspaces.grants", "WS_NOT_FOUND", false),
        ("files.place", "FILE_PLACE_NOT_FOUND", false),
    ] {
        let w = documents_world().await;
        team(&w, Some(FolderAccess::ReadAndChange));
        w.f.f.always(call, code, retryable);
        let outcome = w
            .start(w.request(COMPARE, compare_values(&w)))
            .await
            .unwrap();
        assert!(
            matches!(outcome, StartTaskOutcome::Started { .. }),
            "{call}: {outcome:?}"
        );
        let tasks = w.tasks();
        assert_eq!(tasks.len(), 1, "{call}: the task starts");
        assert!(results_arguments(&tasks[0]).is_empty(), "{call}");
        assert_eq!(
            tasks[0].0.arguments.len(),
            2,
            "{call}: the inputs as before"
        );
    }
}

// ---------------------------------------------------------------- OH-TF-5, OH-ACC-2, OH-DLG-3

/// The person's question about a file in "Docs", as the scenario words it.
async fn ask_about_cv(w: &World) {
    w.f.open().await;
    w.f.say(
        "Tell me more about \u{201c}CV.pdf\u{201d}.",
        vec![doc(w.docs, "CVs/CV.pdf")],
    )
    .await;
}

fn plain_brief(w: &World) -> serde_json::Value {
    json!({
        "agent": w.agent.agent_instance_id.to_string(),
        "brief": "Read CV.pdf (in Docs: CVs/CV.pdf) and say what is in it."
    })
}

async fn question_of(w: &World) -> (BlockId, ordin_service_api::blocks::QuestionBlock) {
    let asked = waiting_of(&w.f, "question");
    assert_eq!(asked.len(), 1, "one folder question waits");
    let row = {
        let mut c = w.f.pool.acquire().await.unwrap();
        crate::store::block(&mut c, asked[0].block_id)
            .await
            .unwrap()
            .unwrap()
    };
    let Block::Question(q) = row.block else {
        panic!("a question")
    };
    (asked[0].block_id, q)
}

async fn answer(w: &World, block_id: BlockId, choice: &str) {
    w.f.host
        .answer_block(
            w.f.ws,
            block_id,
            BlockAnswer::Choice {
                choice: choice.to_owned(),
            },
        )
        .await
        .unwrap();
}

fn turns_with(w: &World, part: &str) -> usize {
    w.f.supervisor
        .turn_texts()
        .iter()
        .filter(|t| t.contains(part))
        .count()
}

// Scenario: Happy path — a question in the person's own words is a plain delegation (OH-TF-5)
// spec: ordin-desktop/components/OrdinHost#OH-TF-5
#[tokio::test]
async fn happy_path_a_question_is_a_plain_delegation_with_no_form() {
    let w = documents_world().await;
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    ask_about_cv(&w).await;
    let e =
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Summarise CV.pdf",
                "command": "/summarise-document",
                "inputs": { "document": "Docs: CVs/CV.pdf" }
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_INPUT_INVALID");
    assert_eq!(
        e.message,
        "The person did not ask for Summarise a document by name. Give the work as a brief, without a command or inputs."
    );
    assert!(waiting_of(&w.f, "form").is_empty(), "no form opens");
    assert!(w.tasks().is_empty());
    let out = w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    assert_eq!(out.text, "Started: Document agent is working on it.");
    let tasks = w.tasks();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].0.command, None);
    assert!(tasks[0].0.free_text.as_deref().unwrap().contains("CV.pdf"));
    assert!(waiting_of(&w.f, "form").is_empty());
}

// Scenario: Edge — a task named in other letters still counts as named (OH-TF-5)
#[tokio::test]
async fn edge_a_task_named_by_title_in_any_case_opens_its_form() {
    let w = documents_world().await;
    w.f.open().await;
    w.f.say(
        "run summarise a document on CV.pdf",
        vec![doc(w.docs, "CVs/CV.pdf")],
    )
    .await;
    let out =
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Summarise CV.pdf",
                "command": "/summarise-document",
                "inputs": { "document": "Docs: CVs/CV.pdf" }
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        out.text,
        "I opened the form for Summarise a document beside the conversation; the person checks it and starts it."
    );
    assert_eq!(waiting_of(&w.f, "form").len(), 1);
    assert!(
        waiting_of(&w.f, "question").is_empty(),
        "no folder question for a named task"
    );
}

// Error-table row: no message from the person yet, so no task is named (OH-TF-5).
#[tokio::test]
async fn error_inputs_before_any_message_of_the_person_are_refused() {
    let w = documents_world().await;
    w.f.open().await;
    let e =
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Summarise it",
                "command": "/summarise-document",
                "inputs": { "document": "Docs: CVs/CV.pdf" }
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_INPUT_INVALID");
    assert!(waiting_of(&w.f, "form").is_empty());
}

// Scenario: Happy path — the agent cannot read the folder, so one card asks (OH-ACC-2)
// spec: ordin-desktop/components/OrdinHost#OH-ACC-2
#[tokio::test]
async fn happy_path_no_grant_shows_the_folder_question() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    let out = w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    assert_eq!(
        out.text,
        "Nothing has started yet. I asked the person to let Document agent read Docs; the task starts when they press Allow. Tell them in one sentence what Document agent will do."
    );
    let (_, q) = question_of(&w).await;
    assert_eq!(q.question, "Document agent needs to read Docs to answer.");
    assert_eq!(q.choices, vec!["Allow".to_owned(), "Not now".to_owned()]);
    assert!(!q.allow_other);
    assert!(w.tasks().is_empty(), "nothing starts");
    assert!(w.grants().is_empty(), "nothing is granted");
    assert!(waiting_of(&w.f, "access").is_empty(), "no tick-box card");
}

// Scenario: Happy path — Allow grants that folder, starts the task, and its answer comes back (OH-ACC-2)
#[tokio::test]
async fn happy_path_allow_grants_the_folder_starts_the_task_and_the_answer_comes_back() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    let (block_id, _) = question_of(&w).await;
    answer(&w, block_id, "Allow").await;
    assert_eq!(
        w.grants(),
        vec![(w.agent.agent_instance_id, w.docs, Some(FolderAccess::Read))],
        "read access to that folder only"
    );
    let tasks = w.tasks();
    assert_eq!(tasks.len(), 1, "the task starts");
    assert_eq!(tasks[0].0.command, None);
    let f = &w.f;
    assert!(
        eventually(|| turns_with(&w, "Started: Document agent is working on it.") == 1).await,
        "Ordin is told the task started"
    );
    let run = *f.supervisor.task_runs.lock().unwrap().last().unwrap();
    f.supervisor.end_with(
        run,
        ordin_runtime_adapter::RunEnd {
            reason: ordin_runtime_adapter::RunEndReason::Completed,
            final_message: Some("The CV is Ana Silva's: ten years in logistics.".to_owned()),
            artefacts: vec![],
        },
    );
    let mut produced = None;
    for _ in 0..200 {
        let bodies: Vec<(String, String)> =
            sqlx::query_as("SELECT body, author FROM ordin_blocks ORDER BY shown_at")
                .fetch_all(&f.pool)
                .await
                .unwrap();
        produced = bodies.into_iter().find(|(b, _)| b.contains("\"produced\""));
        if produced.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let (body, author) = produced.expect("the answer comes back into the conversation");
    assert!(body.contains("The CV is Ana Silva's: ten years in logistics."));
    assert_eq!(author, "ordin");
}

// Scenario: Edge — Not now grants nothing and starts nothing (OH-ACC-2)
#[tokio::test]
async fn edge_not_now_grants_nothing() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    let (block_id, _) = question_of(&w).await;
    answer(&w, block_id, "Not now").await;
    assert!(w.grants().is_empty());
    assert!(!w.holds_grant());
    assert!(w.tasks().is_empty());
    assert!(
        eventually(|| turns_with(
            &w,
            "The person chose Not now: Document agent may not read Docs, so nothing started."
        ) == 1)
        .await
    );
}

// Scenario: Edge — asked once for each message of the person (OH-ACC-2)
#[tokio::test]
async fn edge_the_folder_is_asked_about_once_per_message() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    let (block_id, _) = question_of(&w).await;
    answer(&w, block_id, "Not now").await;
    // The same message: not asked again.
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    assert!(waiting_of(&w.f, "question").is_empty());
    // A new message with the file: asked again.
    w.f.say("Try CV.pdf again", vec![doc(w.docs, "CVs/CV.pdf")])
        .await;
    let out = w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    assert!(
        out.text.starts_with("Nothing has started yet."),
        "{}",
        out.text
    );
    assert_eq!(waiting_of(&w.f, "question").len(), 1);
}

// Edge: a granted folder, or a message with no file, asks nothing (OH-ACC-2).
#[tokio::test]
async fn edge_a_granted_folder_or_no_file_asks_nothing() {
    let w = documents_world().await;
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    ask_about_cv(&w).await;
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    assert!(waiting_of(&w.f, "question").is_empty());
    assert_eq!(w.tasks().len(), 1);
}

// Error-table row: the folders cannot be read, so nothing is asked (OH-ACC-2).
#[tokio::test]
async fn error_the_folders_cannot_be_read_and_nothing_is_asked() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    w.f.f
        .always("workspaces.agent_folders", "WS_DATASTORE_UNAVAILABLE", true);
    let out = w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    assert_eq!(out.text, "Started: Document agent is working on it.");
    assert!(waiting_of(&w.f, "question").is_empty());
}

// Error-table row: the second grant fails on Allow; the first is taken back, nothing starts and
// Ordin is told (OH-ACC-2).
#[tokio::test]
async fn error_a_grant_that_fails_on_allow_takes_back_the_others_and_starts_nothing() {
    let w = documents_world().await;
    let sources = PlaceId::new();
    w.f.workspaces.folders.lock().unwrap().push(AgentFolder {
        place_id: sources,
        name: "Sources".into(),
        read_only: true,
        granted: false,
    });
    w.f.open().await;
    w.f.say(
        "Compare these two",
        vec![doc(w.docs, "a.pdf"), doc(sources, "b.pdf")],
    )
    .await;
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    let (block_id, q) = question_of(&w).await;
    assert_eq!(
        q.question,
        "Document agent needs to read Docs and Sources to answer."
    );
    // The first grant (Docs) is made; the second (Sources) fails; the removal of Docs works.
    w.f.f
        .fail_nth("workspaces.set_grant", 2, "WS_DATASTORE_UNAVAILABLE", true);
    answer(&w, block_id, "Allow").await;
    assert_eq!(
        w.grants(),
        vec![
            (w.agent.agent_instance_id, w.docs, Some(FolderAccess::Read)),
            (w.agent.agent_instance_id, w.docs, None),
        ],
        "Docs is granted, then taken back"
    );
    assert!(w
        .f
        .workspaces
        .folders
        .lock()
        .unwrap()
        .iter()
        .all(|f| !f.granted));
    assert!(w.tasks().is_empty(), "nothing starts");
    assert!(
        eventually(|| turns_with(
            &w,
            "Document agent could not be given access to Docs and Sources (WS_DATASTORE_UNAVAILABLE), so nothing started."
        ) == 1)
        .await
    );
}

// Scenario: Happy path — a listed "/x" command starts under its bare name, by delegation and from
// its form (OH-DLG-3)
// spec: ordin-desktop/components/OrdinHost#OH-DLG-3
#[tokio::test]
async fn happy_path_a_listed_slash_command_starts_under_its_bare_name() {
    let w = documents_world().await;
    w.f.workspaces.folders.lock().unwrap()[0].granted = true;
    w.f.inspector.agents.lock().unwrap()[0]
        .commands
        .push(DirectoryCommand {
            name: "/compare-quotes".into(),
            ..Default::default()
        });
    w.f.open().await;
    for named in ["/compare-quotes", "compare-quotes"] {
        w.f.tool(
            "delegate",
            json!({
                "agent": w.agent.agent_instance_id.to_string(),
                "brief": "Compare the quotes",
                "command": named,
                "arguments": "Kestrel and Acme"
            }),
        )
        .await
        .unwrap();
    }
    let outcome = w
        .start(w.request(SUMMARISE, vec![document("document", w.docs, "a.pdf")]))
        .await
        .unwrap();
    assert!(
        matches!(outcome, StartTaskOutcome::Started { .. }),
        "{outcome:?}"
    );
    let commands: Vec<Option<String>> = w.tasks().iter().map(|t| t.0.command.clone()).collect();
    assert_eq!(
        commands,
        vec![
            Some("compare-quotes".to_owned()),
            Some("compare-quotes".to_owned()),
            Some("summarise-document".to_owned()),
        ]
    );
}

// OH-DLG-3: an install that records a session name runs the stem after its plugin prefix.
#[test]
fn edge_the_run_name_is_the_session_name_without_its_plugin() {
    let mut c = summarise_command();
    assert_eq!(crate::task_form::run_command_name(&c), "summarise-document");
    c.session_name = Some("document-agent:summarise-document".into());
    assert_eq!(crate::task_form::run_command_name(&c), "summarise-document");
}

// OH-TF-4 (amended): an agent with a task that takes inputs carries the named-tasks line.
#[test]
fn edge_the_directory_says_tasks_with_inputs_are_passed_only_when_named() {
    let mut a = agent("Document agent");
    a.commands = vec![summarise_command()];
    let text = crate::seed::directory_text(&[a.clone()], false);
    assert!(
        text.contains(&format!("  {}\n", crate::seed::NAMED_TASKS_LINE)),
        "{text}"
    );
    a.commands = vec![DirectoryCommand {
        name: "compare-quotes".into(),
        ..Default::default()
    }];
    let text = crate::seed::directory_text(&[a], false);
    assert!(!text.contains(crate::seed::NAMED_TASKS_LINE));
}

// Scenario: Edge — the fixed instructions version (OH-TF-6)
// spec: ordin-desktop/components/OrdinHost#OH-TF-6
// Advanced to 1.4.0 by OH-TOP-2 (amended January 2026 (user decision; Approved by the user January 2026)).
// spec: ordin-desktop/components/OrdinHost#OH-TOP-2
#[test]
fn edge_the_fixed_instructions_version_is_1_3_0() {
    assert_eq!(crate::FIXED_INSTRUCTIONS_VERSION, "1.4.0");
}

// Scenario: Happy path — Dismiss on a task's form starts nothing and Ordin is told (OH-DIS-2)
// spec: ordin-desktop/components/OrdinHost#OH-DIS-2
#[tokio::test]
async fn happy_path_dismiss_on_a_task_form_starts_nothing_and_ordin_is_told() {
    let w = documents_world().await;
    let block_id = w.ordin_form(&compare_command(), vec![]).await;
    w.f.host
        .answer_block(w.f.ws, block_id, BlockAnswer::Dismissed)
        .await
        .unwrap();
    assert!(waiting_of(&w.f, "form").is_empty(), "it leaves Needs you");
    assert!(w.tasks().is_empty(), "nothing starts");
    assert!(w.grants().is_empty(), "nothing is granted");
    assert!(
        eventually(|| turns_with(
            &w,
            "The person dismissed Compare documents: nothing was sent and nothing started."
        ) == 1)
        .await,
        "{:?}",
        w.f.supervisor.turn_texts()
    );
    // Its Start button now finds the form answered.
    let mut request = w.request(
        COMPARE,
        vec![
            document("reference", w.docs, "q2.pdf"),
            document("other", w.docs, "q3.pdf"),
        ],
    );
    request.block_id = Some(block_id);
    assert_eq!(
        w.start(request).await.unwrap_err().code(),
        "OH_BLOCK_ANSWERED"
    );
    assert!(w.tasks().is_empty());
}

// Scenario: Edge — Skip on the folder question is its Not now: nothing granted (OH-DIS-3, OH-ACC-2)
// spec: ordin-desktop/components/OrdinHost#OH-DIS-3
#[tokio::test]
async fn edge_skip_on_the_folder_question_grants_nothing() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    w.f.tool("delegate", plain_brief(&w)).await.unwrap();
    let (block_id, _) = question_of(&w).await;
    w.f.host
        .answer_block(w.f.ws, block_id, BlockAnswer::Dismissed)
        .await
        .unwrap();
    assert!(w.grants().is_empty(), "nothing is granted");
    assert!(!w.holds_grant());
    assert!(w.tasks().is_empty(), "nothing starts");
    assert!(
        waiting_of(&w.f, "question").is_empty(),
        "it leaves Needs you"
    );
    assert!(
        eventually(|| turns_with(
            &w,
            "The person chose Not now: Document agent may not read Docs, so nothing started."
        ) == 1)
        .await,
        "{:?}",
        w.f.supervisor.turn_texts()
    );
}

// ---------------------------------------------------------------- OH-ACC-3

fn change_brief(w: &World) -> serde_json::Value {
    json!({
        "agent": w.agent.agent_instance_id.to_string(),
        "brief": "Update the rates table in CV.pdf (in Docs: CVs/CV.pdf).",
        "changes_documents": true
    })
}

// Scenario: Happy path — changing a linked document asks for read and change, and Allow grants it (OH-ACC-3)
// spec: ordin-desktop/components/OrdinHost#OH-ACC-3
#[tokio::test]
async fn happy_path_changing_a_linked_document_asks_for_read_and_change() {
    let w = documents_world().await;
    w.f.files
        .places
        .lock()
        .unwrap()
        .insert(w.docs, "/wc/docs".to_owned());
    ask_about_cv(&w).await;
    w.f.tool("delegate", change_brief(&w)).await.unwrap();
    let (block_id, q) = question_of(&w).await;
    assert_eq!(
        q.question,
        "Document agent needs to change Docs. Changes go back only after you approve them."
    );
    answer(&w, block_id, "Allow").await;
    assert_eq!(
        w.grants(),
        vec![(
            w.agent.agent_instance_id,
            w.docs,
            Some(FolderAccess::ReadAndChange)
        )],
        "read and change on the working copy only"
    );
    assert_eq!(w.tasks().len(), 1, "the task starts");
}

// Scenario: Edge — a place that is not a linked document's working copy is only ever asked for reading (OH-ACC-3)
// spec: ordin-desktop/components/OrdinHost#OH-ACC-3
#[tokio::test]
async fn edge_a_persons_folder_is_never_asked_for_change() {
    let w = documents_world().await;
    ask_about_cv(&w).await;
    w.f.tool("delegate", change_brief(&w)).await.unwrap();
    let (block_id, q) = question_of(&w).await;
    assert_eq!(q.question, "Document agent needs to read Docs to answer.");
    answer(&w, block_id, "Allow").await;
    assert_eq!(
        w.grants(),
        vec![(w.agent.agent_instance_id, w.docs, Some(FolderAccess::Read))]
    );
}
