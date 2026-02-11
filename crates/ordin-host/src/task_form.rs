//! An agent's task started from its form (OrdinHost.md amendment January 2026, OH-TF-1 to OH-TF-4;
//! ADR-119). The person's press of a form's verb arrives as `runs.start_task`; Ordin gives a task its
//! named inputs with `delegate`, or opens its form. Either way the task starts down step 8.3, as a
//! task Ordin starts does, and nothing is sent as a chat message on the person's behalf.

use std::collections::BTreeMap;

use ordin_package_inspector::{
    is_task_date, is_task_number, is_task_whole_number, TaskInput, TaskInputKind, DOCUMENTS_MIN,
    LONG_TEXT_MAX, SHORT_TEXT_MAX,
};
use ordin_service_api::blocks::{Block, FormBlock, FormField, FormInput, TaskFormRef};
use ordin_service_api::commands::ordin::MessageAuthor;
use ordin_service_api::commands::runs::{
    RunsStartTaskInput, TaskInputProblem, TaskInputValue, TaskProblem, TaskValue,
};
use ordin_types::domain::places::{ArtefactLocation, FolderAccess};
use ordin_types::domain::runs::TaskArgument;
use ordin_types::time::Timestamp;
use ordin_types::wire::ContentRef;
use ordin_types::{AgentInstanceId, ArtefactId, BlockId, PlaceId, RunId, WorkspaceId};

use crate::error::OhError;
use crate::host::{lock, Kept, OrdinHost, COMPONENT};
use crate::ports::{AgentFolder, DirectoryAgent, DirectoryCommand, LinkedRepo};
use crate::store;
use crate::tools::{fail, Delegated, Delegated8, Delegation, Stage};
use crate::types::{StartTaskOutcome, TaskInputText};

/// The shipped software-engineering agent: its tasks run in their own worktree (OH-TF-1 step 7 (c)).
pub(crate) const SOFTWARE_ENGINEERING_PROFILE: &str = "urn:ordin:default:workspaces:cspa-431";
/// The most linked repositories the repository question offers (OH-TF-1 step 7 (c)).
const REPOSITORY_QUESTION_MAX: usize = 4;

/// A chosen document, read in step 3: where it is, its name, and what the start records of it.
#[derive(Debug, Clone)]
pub(crate) struct ChosenDocument {
    pub place_id: PlaceId,
    pub relative_path: String,
    pub hash: ordin_types::num::Sha256Hex,
    pub media_type: String,
}

/// What a start from a form carries down step 8.3 to its start (OH-TF-1 steps 5–8).
#[derive(Debug, Clone)]
pub(crate) struct FormStart {
    /// The task's arguments of step 6, one per input with a value, in input order.
    pub arguments: Vec<TaskArgument>,
    /// The chosen documents in field order.
    pub documents: Vec<ChosenDocument>,
    /// The waiting form Ordin opened, when the form was sent from it.
    pub block_id: Option<BlockId>,
    /// The task runs in its own worktree (step 7 (c)).
    pub software_engineering: bool,
    /// True when the person's press started it; false when Ordin did (OH-TF-3).
    pub by_person: bool,
    /// Grant the chosen documents' folders at the start (false for Ordin's start, which never
    /// grants access by itself, OH-ACC-1).
    pub grant: bool,
    pub inputs: u32,
}

/// The values of one task checked against its inputs (step 3).
pub(crate) enum Checked {
    Problems(Vec<TaskInputProblem>),
    Ready {
        arguments: Vec<TaskArgument>,
        documents: Vec<ChosenDocument>,
    },
}

/// The task's inputs; a task without inputs has the one input `arguments`, long text, not required.
pub(crate) fn effective_inputs(command: &DirectoryCommand) -> Vec<TaskInput> {
    match &command.inputs {
        Some(inputs) => inputs.clone(),
        None => vec![TaskInput {
            name: "arguments".to_owned(),
            label: "arguments".to_owned(),
            kind: TaskInputKind::LongText,
            required: false,
            hint: None,
            choices: Vec::new(),
            max: None,
            default: None,
            whole: false,
        }],
    }
}

/// OH-DLG-3: the name a task's command runs under — the bare file stem the Runtime Adapter accepts
/// (RuntimeAdapterInterface.md step 2, `^[a-z0-9][a-z0-9_-]{0,63}$`): the session name without its
/// `<plugin>:` prefix when the install records one, else the command's name without its leading `/`.
/// A command manifest names itself `/summarise-document`; the run is `summarise-document`.
// spec: ordin-desktop/components/OrdinHost#OH-DLG-3
pub(crate) fn run_command_name(command: &DirectoryCommand) -> String {
    match command.session_name.as_deref() {
        Some(session) if !session.trim().is_empty() => session
            .rsplit(':')
            .next()
            .unwrap_or(session)
            .trim_start_matches('/')
            .to_owned(),
        _ => command.name.trim_start_matches('/').to_owned(),
    }
}

/// The task's title: its own, else its name.
pub(crate) fn task_title(command: &DirectoryCommand) -> String {
    command
        .title
        .clone()
        .unwrap_or_else(|| command.name.clone())
}

fn problem(name: &str, problem: TaskProblem) -> TaskInputProblem {
    TaskInputProblem {
        name: name.to_owned(),
        problem,
        document: None,
    }
}

fn kind_fits(kind: TaskInputKind, value: &TaskValue) -> bool {
    matches!(
        (kind, value),
        (
            TaskInputKind::ShortText
                | TaskInputKind::LongText
                | TaskInputKind::Number
                | TaskInputKind::Date,
            TaskValue::Text { .. }
        ) | (TaskInputKind::Document, TaskValue::Document { .. })
            | (TaskInputKind::Documents, TaskValue::Documents { .. })
            | (TaskInputKind::Choice, TaskValue::Choice { .. })
    )
}

/// Text as the task's argument holds it: CR LF and CR as LF, NFC-normalised.
fn argument_text(raw: &str) -> String {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    ordin_types::text::normalise_text(&text).unwrap_or(text)
}

/// The last segment of a relative path: the file name a person knows the document by.
pub(crate) fn file_name(relative_path: &str) -> String {
    relative_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(relative_path)
        .to_owned()
}

/// OH-ATT-1's line for a document: "<file name> (in <link name>: <relative path>)".
fn document_line(folders: &[AgentFolder], doc: &ChosenDocument) -> String {
    let link = folders
        .iter()
        .find(|f| f.place_id == doc.place_id)
        .map(|f| f.name.as_str())
        .unwrap_or("");
    format!(
        "{} (in {link}: {})",
        file_name(&doc.relative_path),
        doc.relative_path
    )
}

/// One input's value, resolved from what was sent and the input's default.
enum Resolved {
    Empty,
    Text(String),
    Documents(Vec<(PlaceId, String)>),
}

impl OrdinHost {
    /// OH-TF-1 step 3: each value against its input. Nothing is written. A document is read to
    /// check it is still in the workspace; a read that fails for another reason is
    /// `OH_DEPENDENCY_FAILED`.
    // spec: ordin-desktop/components/OrdinHost#OH-TF-1
    pub(crate) async fn check_task_values(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        command: &DirectoryCommand,
        values: &[TaskInputValue],
    ) -> Result<Checked, OhError> {
        let inputs = effective_inputs(command);
        let folders = self
            .ports
            .workspaces
            .agent_folders(ws, agent)
            .await
            .map_err(|e| OhError::dependency("workspaces", &e))?;
        let mut problems: Vec<TaskInputProblem> = Vec::new();
        let mut arguments: Vec<TaskArgument> = Vec::new();
        let mut documents: Vec<ChosenDocument> = Vec::new();
        let mut unknown: Vec<TaskInputProblem> = Vec::new();
        for value in values {
            if !inputs.iter().any(|i| i.name == value.name) {
                unknown.push(problem(&value.name, TaskProblem::UnknownInput));
            }
        }
        for input in &inputs {
            let given = values.iter().find(|v| v.name == input.name);
            if let Some(v) = given {
                if !kind_fits(input.kind, &v.value) {
                    problems.push(problem(&input.name, TaskProblem::WrongKind));
                    continue;
                }
            }
            let mut resolved = match given.map(|v| &v.value) {
                None => Resolved::Empty,
                Some(TaskValue::Text { text }) if text.trim().is_empty() => Resolved::Empty,
                Some(TaskValue::Text { text }) => Resolved::Text(text.clone()),
                Some(TaskValue::Choice { choice }) if choice.is_empty() => Resolved::Empty,
                Some(TaskValue::Choice { choice }) => Resolved::Text(choice.clone()),
                Some(TaskValue::Document { document }) => match document {
                    ContentRef::PlaceFile {
                        place_id,
                        relative_path,
                    } => Resolved::Documents(vec![(*place_id, relative_path.clone())]),
                    _ => {
                        problems.push(problem(&input.name, TaskProblem::WrongKind));
                        continue;
                    }
                },
                Some(TaskValue::Documents { documents }) if documents.is_empty() => Resolved::Empty,
                Some(TaskValue::Documents { documents }) => {
                    let mut out = Vec::with_capacity(documents.len());
                    for d in documents {
                        if let ContentRef::PlaceFile {
                            place_id,
                            relative_path,
                        } = d
                        {
                            out.push((*place_id, relative_path.clone()));
                        }
                    }
                    if out.len() != documents.len() {
                        problems.push(problem(&input.name, TaskProblem::WrongKind));
                        continue;
                    }
                    Resolved::Documents(out)
                }
            };
            if matches!(resolved, Resolved::Empty) {
                if let Some(default) = &input.default {
                    resolved = Resolved::Text(default.clone());
                }
            }
            let found = match &resolved {
                Resolved::Empty if input.required => Some(TaskProblem::RequiredEmpty),
                Resolved::Empty => None,
                Resolved::Text(text) => match input.kind {
                    TaskInputKind::ShortText
                        if text.chars().count() > SHORT_TEXT_MAX || text.contains(['\n', '\r']) =>
                    {
                        Some(TaskProblem::TooLong)
                    }
                    TaskInputKind::LongText if text.chars().count() > LONG_TEXT_MAX => {
                        Some(TaskProblem::TooLong)
                    }
                    TaskInputKind::Number if !is_task_number(text.trim()) => {
                        Some(TaskProblem::NotANumber)
                    }
                    // A whole-number input refuses a fraction (AgentModel.md AM-TF-2).
                    TaskInputKind::Number if input.whole && !is_task_whole_number(text.trim()) => {
                        Some(TaskProblem::NotANumber)
                    }
                    TaskInputKind::Date if !is_task_date(text.trim()) => {
                        Some(TaskProblem::NotADate)
                    }
                    TaskInputKind::Choice if !input.choices.contains(text) => {
                        Some(TaskProblem::NotAChoice)
                    }
                    _ => None,
                },
                Resolved::Documents(docs) => {
                    if input.kind == TaskInputKind::Documents && docs.len() < DOCUMENTS_MIN {
                        Some(TaskProblem::TooFewDocuments)
                    } else if input.kind == TaskInputKind::Documents
                        && input
                            .max
                            .is_some_and(|max| docs.len() > usize::try_from(max).unwrap_or(0))
                    {
                        Some(TaskProblem::TooManyDocuments)
                    } else {
                        None
                    }
                }
            };
            if let Some(found) = found {
                problems.push(problem(&input.name, found));
                continue;
            }
            match resolved {
                Resolved::Empty => {}
                Resolved::Text(text) => {
                    let text = match input.kind {
                        TaskInputKind::Number | TaskInputKind::Date => text.trim().to_owned(),
                        _ => text,
                    };
                    arguments.push(TaskArgument {
                        name: input.name.clone(),
                        value: argument_text(&text),
                    });
                }
                Resolved::Documents(docs) => {
                    let mut read = Vec::with_capacity(docs.len());
                    let mut gone = None;
                    for (place_id, relative_path) in docs {
                        if !folders.iter().any(|f| f.place_id == place_id) {
                            gone = Some(file_name(&relative_path));
                            break;
                        }
                        match self.ports.files.read_file(place_id, &relative_path).await {
                            Ok(file) => read.push(ChosenDocument {
                                place_id,
                                relative_path,
                                hash: file.hash,
                                media_type: file.media_type,
                            }),
                            Err(e) if e.code == "FILE_NOT_FOUND" => {
                                gone = Some(file_name(&relative_path));
                                break;
                            }
                            Err(e) => return Err(OhError::dependency("files", &e)),
                        }
                    }
                    if let Some(document) = gone {
                        problems.push(TaskInputProblem {
                            name: input.name.clone(),
                            problem: TaskProblem::DocumentGone,
                            document: Some(document),
                        });
                        continue;
                    }
                    let lines: Vec<String> =
                        read.iter().map(|d| document_line(&folders, d)).collect();
                    arguments.push(TaskArgument {
                        name: input.name.clone(),
                        value: lines.join("\n"),
                    });
                    documents.extend(read);
                }
            }
        }
        problems.extend(unknown);
        if problems.is_empty() {
            Ok(Checked::Ready {
                arguments,
                documents,
            })
        } else {
            Ok(Checked::Problems(problems))
        }
    }

    /// OH-TF-1: starting a task from its form (`runs.start_task`). Steps 1 to 8, in order; a step that
    /// fails ends the call and nothing after it runs.
    // spec: ordin-desktop/components/OrdinHost#OH-TF-1
    pub async fn start_task_from_form(
        &self,
        input: RunsStartTaskInput,
    ) -> Result<StartTaskOutcome, OhError> {
        let ws = input.workspace_id;
        // 1.
        self.ports
            .workspaces
            .ensure_active(ws)
            .await
            .map_err(|_| OhError::WorkspaceUnknown)?;
        // 2.
        let agents = self
            .ports
            .inspector
            .directory(ws)
            .await
            .map_err(|e| OhError::dependency("inspector", &e))?;
        let agent = agents
            .into_iter()
            .find(|a| a.agent_instance_id == input.agent_instance_id)
            .ok_or(OhError::AgentUnknown)?;
        let command = agent
            .commands
            .iter()
            .find(|c| c.command_id == input.command_id)
            .cloned()
            .ok_or(OhError::CommandUnknown)?;
        if command.form_unreadable {
            return Err(OhError::TaskUnreadable);
        }
        // 3.
        let (arguments, documents) = match self
            .check_task_values(ws, agent.agent_instance_id, &command, &input.values)
            .await?
        {
            Checked::Problems(problems) => {
                tracing::info!(
                    component = COMPONENT,
                    workspace_id = ws.to_string().as_str(),
                    agent_instance_id = agent.agent_instance_id.to_string().as_str(),
                    problems = problems.len(),
                    "ordin_host.task_form_refused"
                );
                return Ok(StartTaskOutcome::Refused { problems });
            }
            Checked::Ready {
                arguments,
                documents,
            } => (arguments, documents),
        };
        // 4.
        if let Some(block_id) = input.block_id {
            self.task_form_block(ws, block_id, &agent, &input.command_id)
                .await?;
        }
        // 5–8.
        let inputs = u32::try_from(arguments.len()).unwrap_or(u32::MAX);
        let start = FormStart {
            arguments,
            documents,
            block_id: input.block_id,
            software_engineering: false,
            by_person: true,
            grant: true,
            inputs,
        };
        match self.launch_task(ws, &agent, &command, start).await? {
            Delegated8::Started { run_id, .. } => Ok(StartTaskOutcome::Started { run_id }),
            Delegated8::Waiting(o) => {
                tracing::info!(
                    component = COMPONENT,
                    workspace_id = ws.to_string().as_str(),
                    agent_instance_id = agent.agent_instance_id.to_string().as_str(),
                    "ordin_host.task_form_waiting"
                );
                Ok(StartTaskOutcome::Waiting { text: o.text })
            }
            Delegated8::Failed(f) => Err(OhError::DependencyFailed {
                dependency: "delegation",
                code: f.code,
                retryable: f.retryable,
            }),
        }
    }

    /// OH-TF-1 step 4: the block must be a waiting task form of this agent and task in the
    /// workspace's current conversation.
    async fn task_form_block(
        &self,
        ws: WorkspaceId,
        block_id: BlockId,
        agent: &DirectoryAgent,
        command_id: &str,
    ) -> Result<(), OhError> {
        let (row, current) = {
            let mut c = self.pool.acquire().await?;
            (
                store::block(&mut c, block_id).await?,
                store::current_conversation(&mut c, ws).await?,
            )
        };
        let row = row
            .filter(|r| r.workspace_id == ws)
            .filter(|r| current.as_ref().is_some_and(|c| c.id == r.conversation_id))
            .ok_or(OhError::BlockUnknown)?;
        let names_this_task = match &row.block {
            Block::Form(FormBlock {
                task:
                    Some(TaskFormRef {
                        agent_instance_id,
                        command_id: id,
                    }),
                ..
            }) => *agent_instance_id == agent.agent_instance_id && id == command_id,
            _ => false,
        };
        if !names_this_task {
            return Err(OhError::BlockUnknown);
        }
        if !row.waiting {
            return Err(OhError::BlockAnswered);
        }
        Ok(())
    }

    /// OH-TF-1 step 7 up to step 8.3's start: the agent's identity (a software-engineering task goes
    /// down (c)), `ensure_ready`, OH-ACC-1's folder question for a task with no document, then (a0)
    /// and (b); (a) is skipped, as in OH-TA-1.
    pub(crate) async fn launch_task(
        &self,
        ws: WorkspaceId,
        agent: &DirectoryAgent,
        command: &DirectoryCommand,
        mut start: FormStart,
    ) -> Result<Delegated8, OhError> {
        let (_, identifier, _, _) = self
            .ports
            .catalogue
            .agent_identity(agent.agent_instance_id)
            .await
            .map_err(|e| OhError::dependency("catalogue", &e))?;
        start.software_engineering = identifier == SOFTWARE_ENGINEERING_PROFILE;
        self.ports
            .catalogue
            .ensure_ready(agent.agent_instance_id)
            .await
            .map_err(|e| OhError::dependency("catalogue", &e))?;
        if start.documents.is_empty() {
            if let Some(asked) = self
                .ask_for_access(ws, agent.agent_instance_id, &agent.display_name)
                .await
            {
                return Ok(Delegated8::Waiting(asked));
            }
        }
        let delegation = Delegation {
            workspace_id: ws,
            agent: agent.agent_instance_id,
            name: agent.display_name.clone(),
            args: Delegated {
                brief: task_title(command),
                // OH-DLG-3: the run's command is the bare stem, never the manifest's `/name`.
                command: Some(run_command_name(command)),
                arguments: None,
                artefacts: Vec::new(),
                code_in: None,
                run_again_of: None,
                form: Some(Box::new(start)),
                changes_documents: false,
            },
            stage: Stage::Estimate,
        };
        Ok(self.proceed(delegation, true).await)
    }

    /// OH-TF-1 step 7 (c): the repository a software-engineering task works in — the one holding the
    /// first chosen document, else the only one. `Ok(None)` when the person is asked which one.
    pub(crate) async fn task_repository(
        &self,
        d: &Delegation,
        start: &FormStart,
    ) -> Result<Option<String>, Delegated8> {
        let ws = d.workspace_id;
        let repos = self
            .ports
            .workspaces
            .linked_repositories(ws)
            .await
            .map_err(|e| Delegated8::Failed(crate::tools::dep_fail(&e)))?;
        let mut projects: Vec<LinkedRepo> = Vec::new();
        for repo in repos {
            match self.ports.projects.project_for_place(repo.place_id).await {
                Ok(Some((_, true))) => projects.push(repo),
                Ok(_) => {}
                Err(e) => return Err(Delegated8::Failed(crate::tools::dep_fail(&e))),
            }
        }
        if let Some(repo) = start
            .documents
            .iter()
            .find_map(|doc| projects.iter().find(|r| r.place_id == doc.place_id))
        {
            return Ok(Some(repo.repository.clone()));
        }
        match projects.len() {
            0 => Err(Delegated8::Failed(fail(
                "OH_INPUT_INVALID",
                "Link the project's repository to this workspace and open it as a project first.",
                false,
            ))),
            1 => Ok(Some(projects[0].repository.clone())),
            n if n <= REPOSITORY_QUESTION_MAX => {
                let choices: Vec<String> = projects.iter().map(|r| r.repository.clone()).collect();
                let kept = Delegation {
                    stage: Stage::Repository,
                    ..d.clone()
                };
                if let Err(e) = self
                    .ask(
                        ws,
                        &format!("Which repository should {} work in?", d.name),
                        choices,
                        Some(Kept::Delegation(Box::new(kept))),
                    )
                    .await
                {
                    return Err(Delegated8::Failed(fail(
                        &e.code(),
                        "That did not work; try again.",
                        e.retryable(),
                    )));
                }
                Ok(None)
            }
            _ => Err(Delegated8::Failed(fail(
                "OH_INPUT_INVALID",
                "Choose a document in the repository this task should work in.",
                false,
            ))),
        }
    }

    /// OH-TF-1 step 7, at the start and only then: read access to each chosen document's folder the
    /// agent holds no grant on, then each document recorded as an immediate start's input. Returns
    /// the grants made, for removal if the start fails, and the artefacts in field order.
    pub(crate) async fn grant_and_record(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        start: &FormStart,
    ) -> Result<(Vec<PlaceId>, Vec<ArtefactId>), crate::ports::DepError> {
        let mut made: Vec<PlaceId> = Vec::new();
        if start.grant && !start.documents.is_empty() {
            // Read again now: a grant may have been made while a question waited.
            let folders = self.ports.workspaces.agent_folders(ws, agent).await?;
            for doc in &start.documents {
                let held = folders
                    .iter()
                    .any(|f| f.place_id == doc.place_id && f.granted);
                if held || made.contains(&doc.place_id) {
                    continue;
                }
                if let Err(e) = self
                    .ports
                    .workspaces
                    .set_agent_grant(ws, agent, doc.place_id, Some(FolderAccess::Read))
                    .await
                {
                    self.remove_grants(ws, agent, &made).await;
                    return Err(e);
                }
                made.push(doc.place_id);
            }
        }
        let mut artefacts = Vec::with_capacity(start.documents.len());
        for doc in &start.documents {
            match self
                .ports
                .journal
                .record_artefact(ordin_journal::NewArtefact {
                    location: ArtefactLocation::Place {
                        place_id: doc.place_id,
                    },
                    path: doc.relative_path.clone(),
                    hash: doc.hash.clone(),
                    media_type: doc.media_type.clone(),
                    producing_run_id: None,
                })
                .await
            {
                Ok(id) => artefacts.push(id),
                Err(e) => {
                    self.remove_grants(ws, agent, &made).await;
                    return Err(e);
                }
            }
        }
        Ok((made, artefacts))
    }

    /// OH-ATT-2: at the start of a task given as a brief or a command, each file the person's
    /// latest message attached, in a folder the agent holds a grant on, is read and recorded as
    /// an artefact, so the run's first turn names its full path (DSK-REC-26). A file in a folder the
    /// agent may not read is left out; a file that cannot be read or recorded is left out and
    /// logged. Nothing fails the start.
    // spec: ordin-desktop/components/OrdinHost#OH-ATT-2
    pub(crate) async fn attached_files_as_artefacts(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Vec<ArtefactId> {
        let files: Vec<(PlaceId, String)> = crate::host::lock(&self.state)
            .convs
            .get(&ws)
            .map(|c| c.attached_files.clone())
            .unwrap_or_default();
        if files.is_empty() {
            return Vec::new();
        }
        let folders = match self.ports.workspaces.agent_folders(ws, agent).await {
            Ok(folders) => folders,
            Err(e) => {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.attachment_not_passed"
                );
                return Vec::new();
            }
        };
        let mut artefacts = Vec::new();
        for (place_id, relative_path) in files {
            if !folders.iter().any(|f| f.place_id == place_id && f.granted) {
                continue;
            }
            let file = match self.ports.files.read_file(place_id, &relative_path).await {
                Ok(file) => file,
                Err(e) => {
                    tracing::warn!(
                        component = COMPONENT,
                        code = e.code.as_str(),
                        "ordin_host.attachment_not_passed"
                    );
                    continue;
                }
            };
            match self
                .ports
                .journal
                .record_artefact(ordin_journal::NewArtefact {
                    location: ArtefactLocation::Place { place_id },
                    path: relative_path,
                    hash: file.hash,
                    media_type: file.media_type,
                    producing_run_id: None,
                })
                .await
            {
                Ok(id) => artefacts.push(id),
                Err(e) => tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.attachment_not_passed"
                ),
            }
        }
        artefacts
    }

    /// Removes the grants a failed start made (OH-TF-1 step 7).
    pub(crate) async fn remove_grants(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        made: &[PlaceId],
    ) {
        for place_id in made {
            if let Err(e) = self
                .ports
                .workspaces
                .set_agent_grant(ws, agent, *place_id, None)
                .await
            {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.task_form_grant_not_removed"
                );
            }
        }
    }

    /// OH-TF-1 step 8: the form Ordin opened is answered as step 38 stores an answer ("Form sent"),
    /// and nothing is sent to Ordin as a turn.
    pub(crate) async fn task_form_sent(
        &self,
        ws: WorkspaceId,
        block_id: BlockId,
    ) -> Result<(), OhError> {
        let row = {
            let mut c = self.pool.acquire().await?;
            store::block(&mut c, block_id).await?
        };
        let Some(row) = row else {
            return Ok(());
        };
        {
            let mut tx = self.pool.begin().await?;
            store::insert_message(
                &mut tx,
                row.conversation_id,
                MessageAuthor::Person,
                None,
                "Form sent",
                None,
                Timestamp::now(),
            )
            .await?;
            store::set_block_waiting(&mut tx, block_id, false).await?;
            tx.commit().await?;
        }
        lock(&self.state).kept.remove(&block_id);
        self.unmirror(block_id);
        self.needs_you_changed(ws).await;
        Ok(())
    }

    /// OH-TF-1 step 8's log line.
    pub(crate) fn log_task_started(
        &self,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        run_id: RunId,
        start: &FormStart,
    ) {
        tracing::info!(
            component = COMPONENT,
            workspace_id = ws.to_string().as_str(),
            agent_instance_id = agent.to_string().as_str(),
            run_id = run_id.to_string().as_str(),
            inputs = start.inputs,
            documents = start.documents.len(),
            "ordin_host.task_started_from_form"
        );
    }

    /// OH-TF-3: `delegate` for a command with named inputs. `Ok(Some(output))` answers Ordin; the
    /// task started, or its form waits beside the conversation.
    // spec: ordin-desktop/components/OrdinHost#OH-TF-3
    pub(crate) async fn delegate_with_inputs(
        &self,
        ws: WorkspaceId,
        agent: &DirectoryAgent,
        command: &DirectoryCommand,
        given: &BTreeMap<String, TaskInputText>,
    ) -> Result<Delegated8, OhError> {
        let inputs = effective_inputs(command);
        let folders = self
            .ports
            .workspaces
            .agent_folders(ws, agent.agent_instance_id)
            .await
            .map_err(|e| OhError::dependency("workspaces", &e))?;
        // 2. The values Ordin gave, each read by its input's kind; a key that names no input and a
        // document text that matches no link are left out.
        let place_file = |text: &str| -> Option<ContentRef> {
            let (link, path) = text.split_once(": ")?;
            let folder = folders.iter().find(|f| f.name == link.trim())?;
            let path = path.trim();
            (!path.is_empty()).then(|| ContentRef::PlaceFile {
                place_id: folder.place_id,
                relative_path: path.to_owned(),
            })
        };
        let mut values: Vec<TaskInputValue> = Vec::new();
        for input in &inputs {
            let Some(given) = given.get(&input.name) else {
                continue;
            };
            let texts: Vec<&str> = match given {
                TaskInputText::One(text) => vec![text.as_str()],
                TaskInputText::Many(list) => list.iter().map(String::as_str).collect(),
            };
            let value = match input.kind {
                TaskInputKind::Document => texts
                    .first()
                    .and_then(|t| place_file(t))
                    .map(|document| TaskValue::Document { document }),
                TaskInputKind::Documents => {
                    let documents: Vec<ContentRef> =
                        texts.iter().filter_map(|t| place_file(t)).collect();
                    (!documents.is_empty()).then_some(TaskValue::Documents { documents })
                }
                TaskInputKind::Choice => texts.first().map(|t| TaskValue::Choice {
                    choice: (*t).to_owned(),
                }),
                _ => Some(TaskValue::Text {
                    text: texts.join("\n"),
                }),
            };
            if let Some(value) = value {
                values.push(TaskInputValue {
                    name: input.name.clone(),
                    value,
                });
            }
        }
        // 3. Every check passes and every chosen document lies in a folder the agent may use:
        // the task starts without a form, and no grant is made.
        let checked = self
            .check_task_values(ws, agent.agent_instance_id, command, &values)
            .await?;
        if let Checked::Ready {
            arguments,
            documents,
        } = &checked
        {
            let all_granted = documents.iter().all(|d| {
                folders
                    .iter()
                    .any(|f| f.place_id == d.place_id && f.granted)
            });
            if all_granted {
                let start = FormStart {
                    inputs: u32::try_from(arguments.len()).unwrap_or(u32::MAX),
                    arguments: arguments.clone(),
                    documents: documents.clone(),
                    block_id: None,
                    software_engineering: false,
                    by_person: false,
                    grant: false,
                };
                return self.launch_task(ws, agent, command, start).await;
            }
        }
        // 4. Otherwise the task's form waits beside the conversation, filled with what Ordin gave.
        let title = task_title(command);
        let fields: Vec<FormField> = inputs
            .iter()
            .map(|input| {
                let mine = values.iter().find(|v| v.name == input.name);
                let filled = mine.is_some();
                let text_of = |v: Option<&TaskInputValue>| match v.map(|v| &v.value) {
                    Some(TaskValue::Text { text }) => Some(text.clone()),
                    Some(TaskValue::Choice { choice }) => Some(choice.clone()),
                    _ => None,
                };
                let form_input = match input.kind {
                    TaskInputKind::Document => FormInput::Document {
                        value: match mine.map(|v| &v.value) {
                            Some(TaskValue::Document { document }) => Some(document.clone()),
                            _ => None,
                        },
                    },
                    TaskInputKind::Documents => FormInput::Documents {
                        value: match mine.map(|v| &v.value) {
                            Some(TaskValue::Documents { documents }) => documents.clone(),
                            _ => Vec::new(),
                        },
                    },
                    TaskInputKind::Choice => FormInput::Choice {
                        options: input.choices.clone(),
                        value: text_of(mine).or_else(|| input.default.clone()),
                    },
                    _ => FormInput::Text {
                        value: text_of(mine).or_else(|| input.default.clone()),
                        placeholder: None,
                    },
                };
                FormField {
                    name: input.name.clone(),
                    label: input.label.clone(),
                    required: input.required,
                    input: form_input,
                    filled_by_ordin: filled,
                }
            })
            .collect();
        let block = Block::Form(FormBlock {
            title: title.clone(),
            verb: title.clone(),
            target: ordin_types::wire::run::RunTarget::AgentInstance {
                agent_instance_id: agent.agent_instance_id,
            },
            command: Some(command.name.clone()),
            fields,
            task: Some(TaskFormRef {
                agent_instance_id: agent.agent_instance_id,
                command_id: command.command_id.clone(),
            }),
        });
        self.show_block(ws, MessageAuthor::Ordin, block, true)
            .await?;
        Ok(Delegated8::Waiting(crate::tools::out(
            format!(
                "I opened the form for {title} beside the conversation; the person checks it and starts it."
            ),
            None,
        )))
    }
}
