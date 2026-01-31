//! The `Checker` service (Checker.md *Public Interface* and *Internal Logic* A, E–H).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ordin_agent_model::{
    LockFile, ModelPart, PartKind, PartRef, PartSource, PartStatus, RepositoryRef, Resolution,
    SourceOverride, SourceScope, StagedPart, VocabularyAxis, VocabularySet,
};
use ordin_journal::{Journal, NewJournalEntry};
use ordin_organisation_profile::{OrganisationProfileService, SnapshotSource};
use ordin_resolver::{Conflict, ConflictCandidate, Replacement, ResolveError, ResolverAndCompiler};
use ordin_types::domain::agents::{
    DecisionSource, ExecutablePartDecision, ExecutablePartTrust, WordingChange,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, ExecutablePartDecisionId, PartDraftId};
use sqlx::SqlitePool;

use crate::changes;
use crate::context::{gather, pick_source, resolve_request, scope_of, Item};
use crate::cross::{cross_checks, ingest_rules};
use crate::dossier;
use crate::draft;
use crate::error::{from_source, resolver_failure, CheckError};
use crate::finding::{self, display_name, finding, msg, part_failure, passes};
use crate::log;
use crate::model::*;
use crate::overlay::WithdrawalInForce;
use crate::rehearsal;
use crate::repo::{self, conflict_finding, rebuild_agent, rebuild_failed, Lookup};
use crate::stage_one::{self, retired_terms, PartView};
use crate::store::{self, CandidateRow, RunRow, StoredDecision};
use crate::trust::{self, executable_parts, trust_findings, Declaring};
use crate::wording;

/// The `component` of the Checker's journal records.
const COMPONENT: &str = "checker";

/// Where the Checker reads the trust policy: the organisation profile in the programs, a fixed value
/// in tests. Returns the `agent_catalogue.trust_policy` value and the profile version.
pub(crate) trait TrustSource: Send + Sync {
    fn trust_policy(&self) -> (Option<serde_json::Value>, Option<String>);
}

// spec: ordin-desktop/components/Checker#SA-RECON-06
impl TrustSource for OrganisationProfileService {
    fn trust_policy(&self) -> (Option<serde_json::Value>, Option<String>) {
        let snapshot = self.current();
        let version = match &snapshot.source {
            SnapshotSource::Verified {
                profile_version, ..
            } => Some(profile_version.clone()),
            SnapshotSource::Defaults => None,
        };
        (
            Some(snapshot.document.agent_catalogue.trust_policy.clone()),
            version,
        )
    }
}

pub struct Checker {
    source: OnceLock<Arc<dyn PartSource>>,
    resolver: Arc<ResolverAndCompiler>,
    profile: Arc<dyn TrustSource>,
    journal: Arc<Journal>,
}

/// What one run produced, before it is recorded.
struct Run {
    subject: CheckSubject,
    findings: Vec<Finding>,
    tidied: Vec<WordingChange>,
    version_checked: String,
    agents_that_stop: Vec<String>,
    parts: usize,
    started_at: Timestamp,
    clock: Instant,
}

impl Run {
    fn new(subject: CheckSubject) -> Run {
        Run {
            subject,
            findings: Vec::new(),
            tidied: Vec::new(),
            version_checked: String::new(),
            agents_that_stop: Vec::new(),
            parts: 0,
            started_at: Timestamp::now(),
            clock: Instant::now(),
        }
    }
}

/// `version_checked` holds 1 to 512 characters.
fn version_text(text: String) -> String {
    let text = if text.trim().is_empty() {
        "unknown".to_owned()
    } else {
        text
    };
    text.chars().take(512).collect()
}

/// CHK-PG-1: for each WorkspaceProfile of `items` that sets a `publishGate` and was read from a
/// repository, the Evaluation identifiers of that repository, by the workspace's id. The checks of
/// one agent see only its closure, which never holds the evaluations a gate names.
// spec: ordin-desktop/components/Checker#CHK-PG-1
async fn gate_evaluations(
    source: &dyn PartSource,
    items: &[Item],
) -> Result<BTreeMap<String, Vec<String>>, CheckError> {
    let mut by_repository: Vec<(RepositoryRef, Vec<String>)> = Vec::new();
    let mut out = BTreeMap::new();
    for item in items
        .iter()
        .filter(|i| i.kind == PartKind::WorkspaceProfile && i.manifest.publish_gate().is_some())
    {
        let Some(sourced) = &item.sourced else {
            continue;
        };
        let at = match by_repository
            .iter()
            .position(|(repository, _)| *repository == sourced.repository)
        {
            Some(at) => at,
            None => {
                let parts = source
                    .repository_parts(&sourced.repository)
                    .await
                    .map_err(from_source)?;
                let ids = parts
                    .into_iter()
                    .filter(|p| p.part.kind == PartKind::Evaluation)
                    .map(|p| p.manifest.metadata.id)
                    .collect();
                by_repository.push((sourced.repository.clone(), ids));
                by_repository.len() - 1
            }
        };
        out.insert(item.id().to_owned(), by_repository[at].1.clone());
    }
    Ok(out)
}

/// The parts as the trust derivation sees them.
fn declaring(items: &[Item]) -> Vec<Declaring<'_>> {
    items
        .iter()
        .map(|i| Declaring {
            kind: i.kind,
            manifest: &i.manifest,
            bundled: i
                .sourced
                .as_ref()
                .is_some_and(|s| s.repository == RepositoryRef::Bundled),
        })
        .collect()
}

/// Stage one over items.
fn part_checks(items: &[Item], vocabulary: &VocabularySet) -> Vec<Finding> {
    items
        .iter()
        .flat_map(|item| {
            stage_one::check(
                &PartView {
                    kind: item.kind,
                    manifest: &item.manifest,
                    body: item
                        .files
                        .as_ref()
                        .and_then(|files| stage_one::body_of_files(&item.path, files)),
                },
                vocabulary,
            )
        })
        .collect()
}

/// `installed_part_gone` for a part a plan or lock names that the source no longer has.
fn part_gone(id: &str, detail: String) -> Finding {
    finding(
        "installed_part_gone",
        Severity::Failure,
        Some(id),
        msg::INSTALLED_PART_GONE,
        &[("part", id)],
        vec![Fix::AskOrdin],
        detail,
    )
}

fn ambiguous(id: &str, repositories: usize) -> Finding {
    part_failure(
        "reference_ambiguous",
        id,
        id,
        vec![Fix::AskOrdin],
        format!("{repositories} repositories carry {id}"),
    )
}

/// Each part a plan or lock names, read from the source at its version.
async fn read_named(
    source: &dyn PartSource,
    scope: &SourceScope,
    named: &[PartRef],
    out: &mut Vec<Finding>,
) -> Result<Vec<Item>, CheckError> {
    let mut items = Vec::with_capacity(named.len());
    for part in named {
        match source
            .resolve(scope, part.kind, &part.id, Some(&part.version))
            .await
            .map_err(from_source)?
        {
            Resolution::Found(sourced) => items.push(Item::from_sourced(sourced)),
            Resolution::NotFound => out.push(part_gone(
                &part.id,
                format!(
                    "{}@{} is no longer in its repository",
                    part.id, part.version
                ),
            )),
            Resolution::Ambiguous { repositories } => {
                out.push(ambiguous(&part.id, repositories.len()));
            }
        }
    }
    Ok(items)
}

/// Step 18 among installed parts: two parts filling one `<Kind>/<name>` slot.
fn slot_conflicts(parts: &[PartRef]) -> Vec<Finding> {
    let mut slots: BTreeMap<(PartKind, String), Vec<&PartRef>> = BTreeMap::new();
    for part in parts {
        slots
            .entry((part.kind, part.name.clone()))
            .or_default()
            .push(part);
    }
    slots
        .into_iter()
        .filter(|(_, held)| {
            held.iter()
                .map(|p| (&p.id, &p.version))
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        })
        .map(|((kind, name), held)| {
            let conflict = Conflict {
                slot: format!("{kind:?}/{name}"),
                candidates: held
                    .into_iter()
                    .map(|p| ConflictCandidate {
                        part: p.clone(),
                        requested_by: Vec::new(),
                    })
                    .collect(),
            };
            conflict_finding(&conflict, None)
        })
        .collect()
}

impl Checker {
    /// Built without a source (R18): every operation that reads parts fails with
    /// `CHK_SOURCE_NOT_BOUND` until `install_part_source` has run.
    pub fn new(
        resolver: Arc<ResolverAndCompiler>,
        profile: Arc<OrganisationProfileService>,
        journal: Arc<Journal>,
    ) -> Self {
        Self::with_trust_source(resolver, profile, journal)
    }

    pub(crate) fn with_trust_source(
        resolver: Arc<ResolverAndCompiler>,
        profile: Arc<dyn TrustSource>,
        journal: Arc<Journal>,
    ) -> Self {
        Checker {
            source: OnceLock::new(),
            resolver,
            profile,
            journal,
        }
    }

    /// Bind the source once, after the Catalogue Service is built; a second call fails with
    /// `CHK_SOURCE_ALREADY_BOUND` and leaves the first source in place.
    // spec: ordin-desktop/components/Checker#SA-RECON-01
    pub fn install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), CheckError> {
        self.source
            .set(source)
            .map_err(|_| CheckError::SourceAlreadyBound)
    }

    /// The source of one operation: the request's own, else the bound one.
    fn source_for(
        &self,
        request: Option<&SourceOverride>,
        operation: &str,
    ) -> Result<Arc<dyn PartSource>, CheckError> {
        pick_source(request, self.source.get()).ok_or_else(|| {
            log::source_not_bound(operation);
            CheckError::SourceNotBound
        })
    }

    fn policy(&self) -> TrustPolicy {
        let (value, version) = self.profile.trust_policy();
        trust::parse_policy(value.as_ref(), version.as_deref())
    }

    // The statements are written once, for SQLite, as the sibling table owners do. Release 1 builds the
    // desktop profile; on a PostgreSQL datastore the recorded operations fail with the permanent
    // `CHK_DATASTORE_FAILED` (the Release Builder's `check_parts` uses no datastore).
    fn pool(&self) -> Result<&SqlitePool, CheckError> {
        self.journal
            .datastore()
            .sqlite()
            .ok_or(CheckError::DatastoreFailed { retryable: false })
    }

    /// Writes one journal record; a failure is logged and never fails the operation.
    // The Journal's kind grammar (`^[a-z][a-z0-9_]{0,63}$`) refuses a dot, so record kinds are written
    // as `check_run_completed`, `executable_parts_decided` and `trust_reevaluated` with the component
    // `checker`.
    async fn journal_record(&self, kind: &str, summary: String, data: serde_json::Value) {
        let entry = NewJournalEntry {
            kind: kind.to_owned(),
            component: COMPONENT.to_owned(),
            run_id: None,
            crew_run_id: None,
            workspace_id: None,
            summary,
            data,
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            log::journal_failed(kind, ordin_types::CodedError::code(&error));
        }
    }

    /// Steps 2–4: truncate, record, journal, log. The report is returned only once recorded.
    async fn record(
        &self,
        run: Run,
        candidate: Option<(AgentInstanceId, String, &[ExecutablePart])>,
    ) -> Result<CheckReport, CheckError> {
        let findings = finding::truncate(run.findings);
        let (failures, warnings) = finding::counts(&findings);
        let report = CheckReport {
            check_run_id: CheckRunId(uuid::Uuid::now_v7()),
            subject: run.subject,
            passed: passes(&findings),
            publish_gate_met: !findings.iter().any(|f| f.code == "publish_gate_not_met"),
            findings,
            tidied: run.tidied,
            checked_at: Timestamp::now(),
            version_checked: version_text(run.version_checked),
            agents_that_stop: run.agents_that_stop,
        };
        let candidate = match candidate {
            Some((id, plan_hash, parts)) => Some(CandidateRow {
                agent_instance_id: id,
                plan_hash,
                parts_json: serde_json::to_string(parts)
                    .map_err(|_| CheckError::DatastoreFailed { retryable: false })?,
            }),
            None => None,
        };
        let row = RunRow {
            report: &report,
            started_at: run.started_at,
            failures,
            warnings,
        };
        store::insert_run(self.pool()?, &row, candidate.as_ref()).await?;
        self.journal_record(
            "check_run_completed",
            format!("A check finished: {failures} failures, {warnings} warnings."),
            serde_json::json!({
                "check_run_id": report.check_run_id,
                "subject": report.subject,
                "passed": report.passed,
                "failures": failures,
                "warnings": warnings,
                "tidied": report.tidied.len(),
            }),
        )
        .await;
        log::check_run(&log::RunLine {
            check_run_id: report.check_run_id.to_string(),
            subject_kind: report.subject.kind_text(),
            passed: report.passed,
            failures,
            warnings,
            tidied: report.tidied.len(),
            parts: run.parts,
            duration: run.clock.elapsed(),
        });
        Ok(report)
    }

    /// Logs `checker.check_failed` for an operation that could not run.
    fn failed<T>(subject_kind: &str, result: Result<T, CheckError>) -> Result<T, CheckError> {
        if let Err(error) = &result {
            log::check_failed(subject_kind, error);
        }
        result
    }

    /// Part checks of a plan's closure plus the trust policy; records the run, never the
    /// executable-part decisions (those are `decide_executable_parts`, after install).
    // spec: ordin-desktop/components/Checker#SA-CLOSE-03
    pub async fn check_for_install(
        &self,
        request: &InstallCheck,
    ) -> Result<InstallCheckReport, CheckError> {
        Self::failed("install_candidate", self.for_install(request).await)
    }

    async fn for_install(&self, request: &InstallCheck) -> Result<InstallCheckReport, CheckError> {
        let source = self.source_for(request.source.as_ref(), "check_for_install")?;
        let plan = &request.plan;
        let mut run = Run::new(CheckSubject::InstallCandidate {
            root_id: plan.root.id.clone(),
            plan_hash: plan.plan_hash.clone(),
        });
        run.version_checked = plan.plan_hash.clone();
        let vocabulary = source
            .vocabulary(&request.scope)
            .await
            .map_err(from_source)?;
        let items = read_named(
            source.as_ref(),
            &request.scope,
            &plan.resolved,
            &mut run.findings,
        )
        .await?;
        run.findings.extend(part_checks(&items, &vocabulary));
        let parts = declaring(&items);
        let executable = executable_parts(plan.tool_policy.as_ref(), &parts);
        let policy = self.policy();
        run.findings
            .extend(trust_findings(&policy, &executable, &parts));
        run.parts = items.len();
        let failed = run
            .findings
            .iter()
            .any(|f| f.severity == Severity::Failure && f.code != "trust_needs_it_approval");
        let untrusted: Vec<ExecutablePart> = executable
            .iter()
            .filter(|p| policy.decide(p) == ExecutablePartTrust::NeedsItApproval)
            .cloned()
            .collect();
        let verdict = if failed {
            InstallVerdict::Failed
        } else if !untrusted.is_empty() {
            InstallVerdict::NeedsItApproval { parts: untrusted }
        } else {
            InstallVerdict::Clean
        };
        let candidate = request
            .agent_instance_id
            .map(|id| (id, plan.plan_hash.clone(), executable.as_slice()));
        let report = self.record(run, candidate).await?;
        Ok(InstallCheckReport { verdict, report })
    }

    /// The three handover checks (ADR-092 item 2, ADR-095); not recorded.
    // spec: ordin-desktop/components/Checker#ADR-095
    pub async fn check_handover(
        &self,
        input: &HandoverCheckInput,
    ) -> Result<HandoverCheckReport, CheckError> {
        let outcome = self.handover(input).await;
        Self::failed("handover", outcome)
    }

    async fn handover(
        &self,
        input: &HandoverCheckInput,
    ) -> Result<HandoverCheckReport, CheckError> {
        let mut findings: Vec<HandoverFinding> = input
            .results
            .iter()
            .map(|result| HandoverFinding {
                code: "results_readable".to_owned(),
                passed: !result.local_only && result.readable_by_receiver != Some(false),
                item: Some(result.link.clone()),
                missing_sections: Vec::new(),
            })
            .collect();
        if let Some(contract) = &input.contract {
            let source = self.source_for(None, "check_handover")?;
            let failing = |item: &str| HandoverFinding {
                code: "contract_sections".to_owned(),
                passed: false,
                item: Some(item.to_owned()),
                missing_sections: Vec::new(),
            };
            let resolution = source
                .resolve(
                    &contract.scope,
                    PartKind::OutputContract,
                    &contract.identifier,
                    None,
                )
                .await
                .map_err(from_source)?;
            match resolution {
                Resolution::Found(sourced) => {
                    let part = Item::from_sourced(sourced).model_part();
                    match dossier::sections(&part) {
                        Err(_) => findings.push(failing(&contract.identifier)),
                        Ok(sections) => {
                            for result in &input.results {
                                let Some(headings) = &result.headings else {
                                    findings.push(failing(&result.link));
                                    continue;
                                };
                                let present: BTreeSet<String> =
                                    headings.iter().map(|h| dossier::normalise(h)).collect();
                                let missing: Vec<String> = sections
                                    .iter()
                                    .filter(|s| s.required && !s.derived)
                                    .filter(|s| !present.contains(&dossier::normalise(&s.name)))
                                    .map(|s| s.name.clone())
                                    .collect();
                                findings.push(HandoverFinding {
                                    code: "contract_sections".to_owned(),
                                    passed: missing.is_empty(),
                                    item: Some(result.link.clone()),
                                    missing_sections: missing,
                                });
                            }
                        }
                    }
                }
                Resolution::NotFound | Resolution::Ambiguous { .. } => {
                    findings.push(failing(&contract.identifier));
                }
            }
        }
        findings.push(HandoverFinding {
            code: "receiver_reachable".to_owned(),
            passed: input.receiver.reachable,
            item: None,
            missing_sections: Vec::new(),
        });
        let passed = findings.iter().all(|f| f.passed);
        let failed: Vec<&str> = findings
            .iter()
            .filter(|f| !f.passed)
            .map(|f| f.code.as_str())
            .collect();
        log::handover_checked(passed, &failed);
        Ok(HandoverCheckReport { passed, findings })
    }

    /// Pure: the dossier reader over an OutputContract's sections.
    pub fn read_dossier(
        &self,
        write_up: &str,
        contract: &ModelPart,
    ) -> Result<DossierReading, CheckError> {
        dossier::read(write_up, contract)
    }

    /// Pure: stage one of one part (no I/O, no record).
    pub fn check_part(&self, part: &ModelPart, vocabulary: &VocabularySet) -> Vec<Finding> {
        stage_one::check(&PartView::of(part), vocabulary)
    }

    /// Every part of a repository at a commit, all or nothing (Catalogue Service ingest, ADR-082).
    pub async fn check_repository(
        &self,
        request: &RepositoryCheck,
        subject: CheckSubject,
    ) -> Result<CheckReport, CheckError> {
        let kind = subject.kind_text();
        Self::failed(kind, self.repository(request, subject).await)
    }

    async fn repository(
        &self,
        request: &RepositoryCheck,
        subject: CheckSubject,
    ) -> Result<CheckReport, CheckError> {
        let source = self.source_for(request.source.as_ref(), "check_repository")?;
        let mut run = Run::new(subject);
        run.version_checked = match &run.subject {
            CheckSubject::RepositoryRead { commit, .. } => commit.clone(),
            _ => request.repository_display_name.clone(),
        };
        let gathered = gather(source.as_ref(), request).await?;
        run.findings = repo::repository_findings(
            &self.resolver,
            &source,
            request,
            &gathered,
            &BTreeMap::new(),
        )
        .await?;
        run.parts = gathered.union.len();
        self.record(run, None).await
    }

    pub async fn check_agent_draft(
        &self,
        request: &AgentDraftCheck,
    ) -> Result<CheckReport, CheckError> {
        Self::failed("agent_draft", self.agent_draft(request).await)
    }

    async fn agent_draft(&self, request: &AgentDraftCheck) -> Result<CheckReport, CheckError> {
        let draft = &request.draft;
        if request.parts.is_empty() {
            return Err(CheckError::TooManyFindings);
        }
        let jobs = draft.jobs.len();
        if request
            .job_tasks
            .keys()
            .any(|k| usize::try_from(*k).map_or(true, |k| k >= jobs))
        {
            return Err(CheckError::InputInvalid { field: "job_tasks" });
        }
        // A draft whose parts hold no WorkspaceProfile has no agent to check, so it is refused as a
        // malformed request (`CHK_INPUT_INVALID`).
        let Some(root_at) = request
            .parts
            .iter()
            .position(|s| s.part.kind == PartKind::WorkspaceProfile)
        else {
            return Err(CheckError::InputInvalid { field: "parts" });
        };
        let source = self.source_for(request.target.source.as_ref(), "check_agent_draft")?;
        let mut run = Run::new(CheckSubject::AgentDraft {
            agent_draft_id: draft.id,
        });
        run.version_checked = if draft.branch.trim().is_empty() {
            "draft".to_owned()
        } else {
            format!("draft branch {}", draft.branch)
        };

        // Step 14: the wording rules, before every other check.
        let mut fields: Vec<(String, String)> = vec![
            ("name".to_owned(), draft.name.clone()),
            ("purpose".to_owned(), draft.purpose.clone()),
        ];
        let lists: [(&str, Vec<String>); 5] = [
            ("jobs", draft.jobs.iter().map(|j| j.text.clone()).collect()),
            ("tasks", draft.tasks.clone()),
            ("will", draft.will.clone()),
            ("wont", draft.wont.clone()),
            ("needs", draft.needs.clone()),
        ];
        for (list, items) in &lists {
            for (i, text) in items.iter().enumerate() {
                fields.push((format!("{list}[{i}]"), text.clone()));
            }
        }
        let mut parts: Vec<StagedPart> = request.parts.clone();
        for staged in &parts {
            let metadata = &staged.part.manifest.metadata;
            if !metadata.description.is_empty() {
                fields.push((
                    format!("parts.{}.description", metadata.id),
                    metadata.description.clone(),
                ));
            }
        }
        let (tidied, changes, wording_findings) = wording::apply(&fields, &request.agent_language);
        let tidied: BTreeMap<String, String> = tidied.into_iter().collect();
        for staged in &mut parts {
            let key = format!("parts.{}.description", staged.part.manifest.metadata.id);
            if let Some(text) = tidied.get(&key) {
                staged.part.manifest.metadata.description = text.clone();
            }
        }
        let text_of = |key: &str| tidied.get(key).cloned().unwrap_or_default();
        let name = text_of("name");
        let tidied_list = |list: &str, n: usize| -> Vec<String> {
            (0..n).map(|i| text_of(&format!("{list}[{i}]"))).collect()
        };
        let job_texts = tidied_list("jobs", draft.jobs.len());
        let task_texts = tidied_list("tasks", draft.tasks.len());
        run.tidied = changes;
        run.findings.extend(wording_findings);
        run.findings
            .extend(wording::task_title_verbs(&parts, &request.agent_language));

        // Step 15.
        run.findings.extend(draft::mapping(&request.mapping_report));

        // Step 16: sections C and D over the staged parts in the target repository.
        let mut target = request.target.clone();
        let shadowed: BTreeSet<(PartKind, String)> = parts
            .iter()
            .map(|s| (s.part.kind, s.part.manifest.metadata.id.clone()))
            .collect();
        target
            .changes
            .retain(|s| !shadowed.contains(&(s.part.kind, s.part.manifest.metadata.id.clone())));
        target.changes.extend(parts.iter().cloned());
        let gathered = gather(source.as_ref(), &target).await?;
        run.findings.extend(
            repo::repository_findings(
                &self.resolver,
                &source,
                &target,
                &gathered,
                &request.reachable_places,
            )
            .await?,
        );
        let retired: Vec<(VocabularyAxis, String)> = gathered
            .union
            .iter()
            .filter(|i| i.kind == PartKind::Vocabulary)
            .flat_map(|i| retired_terms(&i.manifest))
            .collect();
        run.findings
            .extend(draft::retired_citations(&parts, &retired));

        // Step 17: the trust policy over the staged parts.
        let staged_items: Vec<Item> = parts.iter().map(Item::from_staged).collect();
        let declared = declaring(&staged_items);
        let executable = executable_parts(None, &declared);
        run.findings
            .extend(trust_findings(&self.policy(), &executable, &declared));

        // Step 18: conflicts across every pack and overlay the agent allows.
        let root = parts[root_at].part.manifest.clone();
        let resolve = resolve_request(
            &target.scope,
            &root.metadata.id,
            parts.clone(),
            Vec::new(),
            true,
            &source,
        );
        let outcome = async {
            let space = self.resolver.pack_space(&resolve).await?;
            self.resolver.conflicts(&resolve, &space).await
        }
        .await;
        match outcome {
            Ok(conflicts) => run
                .findings
                .extend(conflicts.iter().map(|c| conflict_finding(c, None))),
            Err(error) => {
                if let Some(stop) = resolver_failure(&error) {
                    return Err(stop);
                }
                run.findings.extend(repo::resolve_error_findings(
                    &root.metadata.id,
                    display_name(&name, &root.metadata.id),
                    None,
                    &error,
                ));
            }
        }

        // Step 19: the maker's fixed packs settle each required rule.
        let mut packs: Vec<draft::PackClass> = source
            .ingredients(&target.scope)
            .await
            .map_err(from_source)?
            .into_iter()
            .filter(|p| p.part.kind == PartKind::PracticePack)
            .map(|p| (p.part.identifier, p.manifest.metadata.class))
            .collect();
        packs.extend(
            gathered
                .union
                .iter()
                .filter(|i| i.kind == PartKind::PracticePack)
                .map(|i| (i.id().to_owned(), i.manifest.metadata.class.clone())),
        );
        run.findings
            .extend(draft::fixed_packs(&root, &packs, &gathered.vocabulary));

        // Steps 20–22.
        run.findings.extend(draft::jobs_covered(
            &job_texts,
            &task_texts,
            &draft.tasks,
            &request.job_tasks,
        ));
        run.findings.extend(draft::name_collision(
            &name,
            &request.repository_agent_names,
        ));
        run.findings.extend(draft::format(
            request.target_format,
            &request.loss_report,
            &request.borrowed,
        ));

        // Step 23: borrowed parts reachable from the target repository's own scope.
        let own = scope_of(&target.repository, &target.scope);
        for id in &request.borrowed {
            let resolution = source
                .resolve_any_kind(&own, id)
                .await
                .map_err(from_source)?;
            match resolution {
                Resolution::Found(sourced) => {
                    if sourced.part.status == PartStatus::Deprecated {
                        run.findings.push(finding(
                            "deprecated_part_reused",
                            Severity::Warning,
                            Some(id),
                            msg::PART_INVALID,
                            &[("part", display_name(&sourced.part.name, id))],
                            Vec::new(),
                            format!("{id} is deprecated"),
                        ));
                    }
                }
                Resolution::NotFound | Resolution::Ambiguous { .. } => {
                    run.findings.push(finding(
                        "borrowed_unreachable",
                        Severity::Failure,
                        Some(id),
                        msg::BORROWED_UNREACHABLE,
                        &[("part", id)],
                        vec![Fix::AskAgentMaker],
                        format!("{id} cannot be reached from the target repository's scope"),
                    ));
                }
            }
        }

        // Steps 24–25.
        run.findings
            .extend(draft::bare_commands(&parts, &root.slug()));
        run.findings
            .extend(draft::publish_gate(&root, request.rehearsal.as_deref()));
        run.parts = gathered.union.len();
        self.record(run, None).await
    }

    pub async fn check_part_draft(
        &self,
        request: &RepositoryCheck,
        part_draft_id: PartDraftId,
        reachable_places: &BTreeMap<String, bool>,
    ) -> Result<CheckReport, CheckError> {
        Self::failed(
            "part_draft",
            self.part_draft(request, part_draft_id, reachable_places)
                .await,
        )
    }

    async fn part_draft(
        &self,
        request: &RepositoryCheck,
        part_draft_id: PartDraftId,
        reachable_places: &BTreeMap<String, bool>,
    ) -> Result<CheckReport, CheckError> {
        if request.changes.is_empty() && request.deletions.is_empty() {
            return Err(CheckError::TooManyFindings);
        }
        let source = self.source_for(request.source.as_ref(), "check_part_draft")?;
        let mut run = Run::new(CheckSubject::PartDraft { part_draft_id });
        run.version_checked = format!("draft in {}", request.repository_display_name);
        // Step 14 over each changed part's description.
        let fields: Vec<(String, String)> = request
            .changes
            .iter()
            .filter(|s| !s.part.manifest.metadata.description.is_empty())
            .map(|s| {
                (
                    format!("parts.{}.description", s.part.manifest.metadata.id),
                    s.part.manifest.metadata.description.clone(),
                )
            })
            .collect();
        let (tidied, changes, findings) = wording::apply(&fields, "en-GB");
        let tidied: BTreeMap<String, String> = tidied.into_iter().collect();
        let mut request = request.clone();
        for staged in &mut request.changes {
            let key = format!("parts.{}.description", staged.part.manifest.metadata.id);
            if let Some(text) = tidied.get(&key) {
                staged.part.manifest.metadata.description = text.clone();
            }
        }
        run.tidied = changes;
        run.findings = findings;
        run.findings
            .extend(wording::task_title_verbs(&request.changes, "en-GB"));
        let gathered = gather(source.as_ref(), &request).await?;
        run.findings.extend(
            repo::repository_findings(
                &self.resolver,
                &source,
                &request,
                &gathered,
                reachable_places,
            )
            .await?,
        );
        run.parts = gathered.union.len();
        self.record(run, None).await
    }

    pub async fn check_withdrawal(
        &self,
        request: &WithdrawalCheck,
        part_draft_id: PartDraftId,
    ) -> Result<CheckReport, CheckError> {
        Self::failed("withdrawal", self.withdrawal(request, part_draft_id).await)
    }

    // spec: ordin-desktop/components/Checker#UI-D99
    async fn withdrawal(
        &self,
        request: &WithdrawalCheck,
        part_draft_id: PartDraftId,
    ) -> Result<CheckReport, CheckError> {
        let entry = &request.entry;
        if entry.reason.trim().is_empty() && request.proposed_list.is_empty() {
            return Err(CheckError::InputInvalid {
                field: "entry.reason",
            });
        }
        let repository = &request.repository;
        let source = self.source_for(repository.source.as_ref(), "check_withdrawal")?;
        let mut run = Run::new(CheckSubject::Withdrawal { part_draft_id });
        run.version_checked = format!("withdrawal of {}", entry.part_identifier);
        let gathered = gather(source.as_ref(), repository).await?;
        run.parts = gathered.union.len();
        run.findings.extend(changes::list_form(
            entry,
            &request.existing_entries,
            &request.proposed_list,
        ));
        run.findings
            .extend(changes::target_form(entry, &gathered.union));

        // The withdrawn part as the source holds it, for its users.
        let withdrawn = gathered
            .union
            .iter()
            .find(|i| {
                i.kind == entry.part_kind
                    && i.id() == entry.part_identifier
                    && entry
                        .part_version
                        .as_ref()
                        .is_none_or(|v| *v == i.manifest.metadata.version)
            })
            .and_then(|i| i.sourced.clone());
        let users = match &withdrawn {
            Some(part) => source.used_by(part).await.map_err(from_source)?,
            None => Vec::new(),
        };

        // The replacement: same kind, neither deprecated nor withdrawn, reachable from every user.
        let mut replacement_ok = false;
        if let Some(replacement) = &entry.replacement_identifier {
            let version = entry.replacement_version.as_deref();
            let mut problem: Option<String> = None;
            let found = source
                .resolve(&repository.scope, entry.part_kind, replacement, version)
                .await
                .map_err(from_source)?;
            match found {
                Resolution::Found(part) => {
                    let withdrawn_already = part.part.withdrawn
                        || request.existing_entries.iter().any(|e| {
                            e.part_kind == entry.part_kind
                                && e.part_identifier == *replacement
                                && e.part_version
                                    .as_ref()
                                    .is_none_or(|v| *v == part.part.version)
                        });
                    if part.part.status == PartStatus::Deprecated || withdrawn_already {
                        problem = Some(format!("{replacement} is deprecated or withdrawn"));
                    }
                }
                _ => problem = Some(format!("{replacement} is not a part of the same kind")),
            }
            if problem.is_none() {
                for user in &users {
                    let scope = scope_of(&user.repository, &repository.scope);
                    let reach = source
                        .resolve(&scope, entry.part_kind, replacement, version)
                        .await
                        .map_err(from_source)?;
                    if !matches!(reach, Resolution::Found(_)) {
                        problem = Some(format!(
                            "{replacement} cannot be reached from {}",
                            user.part.identifier
                        ));
                        break;
                    }
                }
            }
            match problem {
                Some(detail) => run
                    .findings
                    .push(changes::replacement_invalid(entry, detail)),
                None => replacement_ok = true,
            }
        }

        // Every agent that uses the part, rebuilt with the replacement: a warning, never a failure.
        let in_force: Arc<dyn PartSource> = Arc::new(WithdrawalInForce {
            inner: Arc::clone(&source),
            entry: entry.clone(),
        });
        for user in &users {
            let id = user.part.identifier.clone();
            let name = display_name(&user.part.name, &id).to_owned();
            let stops = match (&entry.replacement_identifier, replacement_ok) {
                (Some(replacement), true) => {
                    let scope = scope_of(&user.repository, &repository.scope);
                    let resolve = resolve_request(
                        &scope,
                        &id,
                        Vec::new(),
                        vec![Replacement {
                            withdrawn_id: entry.part_identifier.clone(),
                            withdrawn_kind: entry.part_kind,
                            replacement_id: replacement.clone(),
                            replacement_version: entry.replacement_version.clone(),
                        }],
                        false,
                        &in_force,
                    );
                    let findings = rebuild_agent(&self.resolver, &resolve, &id, &name).await?;
                    findings
                        .iter()
                        .find(|f| f.severity == Severity::Failure)
                        .map(|f| f.detail.clone())
                }
                _ => Some(format!("{id} has no replacement for the withdrawn part")),
            };
            if let Some(detail) = stops {
                run.agents_that_stop.push(id.clone());
                run.findings.push(changes::agent_stops(&id, &name, detail));
            }
        }
        self.record(run, None).await
    }

    pub async fn check_vocabulary(
        &self,
        request: &VocabularyCheck,
        part_draft_id: PartDraftId,
    ) -> Result<CheckReport, CheckError> {
        Self::failed(
            "vocabulary_change",
            self.vocabulary(request, part_draft_id).await,
        )
    }

    async fn vocabulary(
        &self,
        request: &VocabularyCheck,
        part_draft_id: PartDraftId,
    ) -> Result<CheckReport, CheckError> {
        let repository = &request.repository;
        let source = self.source_for(repository.source.as_ref(), "check_vocabulary")?;
        let mut run = Run::new(CheckSubject::VocabularyChange { part_draft_id });
        run.version_checked = format!(
            "vocabulary change in {}",
            repository.repository_display_name
        );
        let current = source
            .vocabulary(&repository.scope)
            .await
            .map_err(from_source)?;
        let gathered = gather(source.as_ref(), repository).await?;
        run.findings.extend(changes::vocabulary_form(
            request.axis,
            &request.added,
            &request.retired,
            &current,
            &gathered.before,
        ));
        for code in &request.retired {
            let citing = source
                .citing_parts(&repository.repository, request.axis, code)
                .await
                .map_err(from_source)?;
            let elsewhere: Vec<String> = citing
                .iter()
                .filter(|p| p.repository != repository.repository)
                .map(|p| display_name(&p.part.name, &p.part.identifier).to_owned())
                .collect();
            if !elsewhere.is_empty() {
                run.findings
                    .push(changes::cited_elsewhere(code, &elsewhere));
            }
        }
        // Then section D for the whole repository with the changed vocabulary part.
        run.findings.extend(
            repo::repository_findings(
                &self.resolver,
                &source,
                repository,
                &gathered,
                &BTreeMap::new(),
            )
            .await?,
        );
        run.parts = gathered.union.len();
        self.record(run, None).await
    }

    /// **Check it** on an installed agent, against its lock file (ADR-094 item 3).
    pub async fn check_installed(
        &self,
        id: AgentInstanceId,
        lock: &LockFile,
        scope: &SourceScope,
    ) -> Result<CheckReport, CheckError> {
        Self::failed("installed_agent", self.installed(id, lock, scope).await)
    }

    // spec: ordin-desktop/components/Checker#ADR-094
    async fn installed(
        &self,
        id: AgentInstanceId,
        lock: &LockFile,
        scope: &SourceScope,
    ) -> Result<CheckReport, CheckError> {
        let source = self.source_for(None, "check_installed")?;
        let mut run = Run::new(CheckSubject::InstalledAgent {
            agent_instance_id: id,
        });
        run.version_checked = lock.plan_hash.clone();
        let plan = match self.resolver.resolve_lock(lock, scope).await {
            Ok(plan) => Some(plan),
            Err(error) => {
                if let Some(stop) = resolver_failure(&error) {
                    return Err(stop);
                }
                match &error {
                    // Reported per part below, from the source's own answer.
                    ResolveError::LockPartUnavailable { .. }
                    | ResolveError::PartWithdrawn { .. } => {}
                    other => run.findings.push(rebuild_failed(
                        &lock.composition.id,
                        &lock.composition.name,
                        other,
                    )),
                }
                None
            }
        };
        // CHK-INS-2: the lock may list its composition again among its artefacts; each part is read once.
        // spec: ordin-desktop/components/Checker#CHK-INS-2
        let mut named: Vec<PartRef> = Vec::with_capacity(lock.artefacts.len() + 1);
        for part in std::iter::once(&lock.composition).chain(&lock.artefacts) {
            if !named
                .iter()
                .any(|n| n.kind == part.kind && n.id == part.id && n.version == part.version)
            {
                named.push(part.clone());
            }
        }
        let items = read_named(source.as_ref(), scope, &named, &mut run.findings).await?;
        let withdrawn_entries = source.withdrawn(scope).await.map_err(from_source)?;
        for item in items
            .iter()
            .filter(|i| i.sourced.as_ref().is_some_and(|s| s.part.withdrawn))
        {
            let replacement = withdrawn_entries
                .iter()
                .find(|e| e.part_kind == item.kind && e.part_identifier == item.id())
                .and_then(|e| e.replacement_identifier.clone());
            let fixes = match &replacement {
                Some(to) => vec![Fix::FixIt {
                    action: FixAction::PointToReplacement {
                        field: item.id().to_owned(),
                        from: item.id().to_owned(),
                        to: to.clone(),
                    },
                }],
                None => vec![Fix::AskOrdin],
            };
            run.findings.push(finding(
                "installed_part_withdrawn",
                Severity::Failure,
                Some(item.id()),
                msg::INSTALLED_PART_WITHDRAWN,
                &[("part", item.name())],
                fixes,
                format!("{} has been withdrawn", item.id()),
            ));
        }
        let vocabulary = source.vocabulary(scope).await.map_err(from_source)?;
        run.findings.extend(part_checks(&items, &vocabulary));
        let refs: Vec<&Item> = items.iter().collect();
        // CHK-INS-1: offers the lock did not choose are choices, not references.
        let chosen: std::collections::BTreeSet<String> = lock
            .selections
            .overlays
            .iter()
            .chain(&lock.selections.practice_packs)
            .map(|p| p.id.clone())
            .chain(lock.overlays_applied.iter().map(|p| p.id.clone()))
            .collect();
        run.findings
            .extend(repo::references_chosen(&refs, &items, None, Some(&chosen)).await?);
        let parts = declaring(&items);
        let executable =
            executable_parts(plan.as_ref().and_then(|p| p.tool_policy.as_ref()), &parts);
        run.findings
            .extend(trust_findings(&self.policy(), &executable, &parts));
        run.findings.extend(slot_conflicts(&named));
        run.parts = items.len();
        self.record(run, None).await
    }

    /// **Check it** on a library agent or part, against the protected branch.
    pub async fn check_library_item(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        identifier: &str,
    ) -> Result<CheckReport, CheckError> {
        Self::failed(
            "library_item",
            self.library_item(scope, kind, identifier).await,
        )
    }

    async fn library_item(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        identifier: &str,
    ) -> Result<CheckReport, CheckError> {
        let source = self.source_for(None, "check_library_item")?;
        let found = source
            .resolve(scope, kind, identifier, None)
            .await
            .map_err(from_source)?;
        let sourced = match found {
            Resolution::Found(sourced) => sourced,
            Resolution::NotFound => {
                return self
                    .item_missing(
                        identifier,
                        part_failure(
                            "reference_unresolved",
                            identifier,
                            identifier,
                            vec![Fix::AskOrdin],
                            format!("{identifier} cannot be found"),
                        ),
                    )
                    .await
            }
            Resolution::Ambiguous { repositories } => {
                return self
                    .item_missing(identifier, ambiguous(identifier, repositories.len()))
                    .await
            }
        };
        let mut run = Run::new(CheckSubject::LibraryItem {
            part_identifier: identifier.to_owned(),
            version: sourced.part.version.clone(),
        });
        run.version_checked = sourced.part.version.clone();
        let vocabulary = source.vocabulary(scope).await.map_err(from_source)?;
        let root = Item::from_sourced(sourced);
        let lookup = Lookup {
            source: source.as_ref(),
            scope,
        };
        if kind != PartKind::WorkspaceProfile {
            let one = [root];
            run.findings.extend(part_checks(&one, &vocabulary));
            let refs: Vec<&Item> = one.iter().collect();
            run.findings
                .extend(repo::references(&refs, &one, Some(&lookup)).await?);
            run.parts = 1;
            return self.record(run, None).await;
        }
        let resolve = resolve_request(scope, identifier, Vec::new(), Vec::new(), false, &source);
        let plan = match self.resolver.resolve(&resolve).await {
            Ok(plan) => plan,
            Err(error) => {
                if let Some(stop) = resolver_failure(&error) {
                    return Err(stop);
                }
                let one = [root];
                run.findings.extend(part_checks(&one, &vocabulary));
                run.findings.extend(repo::resolve_error_findings(
                    identifier,
                    one[0].name(),
                    Some(identifier),
                    &error,
                ));
                run.parts = 1;
                return self.record(run, None).await;
            }
        };
        let closure = read_named(source.as_ref(), scope, &plan.resolved, &mut run.findings).await?;
        run.findings.extend(part_checks(&closure, &vocabulary));
        let refs: Vec<&Item> = closure.iter().collect();
        run.findings
            .extend(repo::references(&refs, &closure, Some(&lookup)).await?);
        let gates = gate_evaluations(source.as_ref(), &closure).await?;
        run.findings.extend(cross_checks(&closure, &gates));
        run.findings.extend(ingest_rules(&closure, &vocabulary));
        let parts = declaring(&closure);
        let executable = executable_parts(plan.tool_policy.as_ref(), &parts);
        run.findings
            .extend(trust_findings(&self.policy(), &executable, &parts));
        run.findings
            .extend(rebuild_agent(&self.resolver, &resolve, identifier, root.name()).await?);
        run.parts = closure.len();
        self.record(run, None).await
    }

    /// A library item the source does not hold: one finding, recorded.
    async fn item_missing(
        &self,
        identifier: &str,
        why: Finding,
    ) -> Result<CheckReport, CheckError> {
        let mut run = Run::new(CheckSubject::LibraryItem {
            part_identifier: identifier.to_owned(),
            version: String::new(),
        });
        run.version_checked = "not found".to_owned();
        run.findings.push(why);
        self.record(run, None).await
    }

    /// Pure: each expectation against the run's evidence.
    pub fn check_rehearsal(
        &self,
        rules: &[ExpectationRule],
        evidence: &RunEvidence,
    ) -> Vec<ExpectationOutcome> {
        rehearsal::check(rules, evidence)
    }

    /// Pure: the wording rules over named text fields; returns the tidied text and the changes.
    pub fn apply_wording_rules(
        &self,
        fields: &[(String, String)],
        language: &str,
    ) -> (Vec<(String, String)>, Vec<WordingChange>, Vec<Finding>) {
        wording::apply(fields, language)
    }

    /// Decides and stores the trust of each executable part of an instance (install, update).
    // spec: ordin-desktop/components/Checker#SA-RECON-06
    pub async fn decide_executable_parts(
        &self,
        id: AgentInstanceId,
        parts: &[ExecutablePart],
    ) -> Result<Vec<ExecutablePartDecision>, CheckError> {
        let policy = self.policy();
        let decided_at = Timestamp::now();
        let mut seen = BTreeSet::new();
        let rows: Vec<StoredDecision> = parts
            .iter()
            .filter(|p| seen.insert((p.part_name.clone(), trust::kind_text(p.kind))))
            .map(|p| StoredDecision {
                decision: ExecutablePartDecision {
                    id: ExecutablePartDecisionId::new(),
                    agent_instance_id: id,
                    part_name: p.part_name.clone(),
                    kind: p.kind,
                    decision: policy.decide(p),
                    source: DecisionSource::OrganisationPolicy,
                    decided_at,
                },
                declared_by: p.declared_by.clone(),
            })
            .collect();
        store::replace_decisions(self.pool()?, id, &rows).await?;
        let trusted = rows
            .iter()
            .filter(|r| r.decision.decision == ExecutablePartTrust::Trusted)
            .count();
        let needs = rows.len() - trusted;
        self.journal_record(
            "executable_parts_decided",
            format!("Trust decided for {} executable parts.", rows.len()),
            serde_json::json!({
                "agent_instance_id": id,
                "trusted": trusted,
                "needs_it_approval": needs,
            }),
        )
        .await;
        log::executable_parts_decided(&id.to_string(), trusted, needs);
        Ok(rows.into_iter().map(|r| r.decision).collect())
    }

    /// Every executable part (trusted or not) derived at the instance's latest `check_for_install`
    /// with `agent_instance_id = Some(instance)` (SA-CLOSE-03); none stored → `CHK_INSTANCE_UNKNOWN`.
    pub async fn executable_parts(
        &self,
        instance: AgentInstanceId,
    ) -> Result<Vec<ExecutablePart>, CheckError> {
        let stored = store::load_candidate(self.pool()?, instance)
            .await?
            .ok_or(CheckError::InstanceUnknown { id: instance })?;
        serde_json::from_str(&stored).map_err(|_| CheckError::DatastoreFailed { retryable: false })
    }

    /// Re-decides every stored instance against the current profile; returns the instances whose
    /// trust was lost (each to be disabled until IT approves) and those whose trust was gained.
    // spec: ordin-desktop/components/Checker#ADR-082
    pub async fn reevaluate_trust(&self) -> Result<TrustChanges, CheckError> {
        let pool = self.pool()?;
        let policy = self.policy();
        let instances = store::decided_instances(pool).await?;
        let mut changes = TrustChanges::default();
        let now = Timestamp::now();
        for instance in &instances {
            let mut changed = Vec::new();
            let (mut lost, mut gained) = (false, false);
            // CHK-TR-1: the parts the instance's stored candidate marks as bundled stay trusted.
            let bundled: BTreeSet<(String, &str)> =
                match store::load_candidate(pool, *instance).await? {
                    Some(stored) => serde_json::from_str::<Vec<ExecutablePart>>(&stored)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|p| p.bundled)
                        .map(|p| (p.part_name, trust::kind_text(p.kind)))
                        .collect(),
                    None => BTreeSet::new(),
                };
            for row in store::load_decisions(pool, *instance).await? {
                let part = ExecutablePart {
                    part_name: row.decision.part_name.clone(),
                    kind: row.decision.kind,
                    declared_by: row.declared_by.clone(),
                    bundled: bundled.contains(&(
                        row.decision.part_name.clone(),
                        trust::kind_text(row.decision.kind),
                    )),
                };
                let decision = policy.decide(&part);
                if decision != row.decision.decision {
                    match decision {
                        ExecutablePartTrust::NeedsItApproval => lost = true,
                        ExecutablePartTrust::Trusted => gained = true,
                    }
                    changed.push(ExecutablePartDecision {
                        decision,
                        decided_at: now,
                        ..row.decision
                    });
                }
            }
            store::update_decisions(pool, &changed).await?;
            if lost {
                changes.lost.push(*instance);
            } else if gained {
                changes.gained.push(*instance);
            }
        }
        self.journal_record(
            "trust_reevaluated",
            format!(
                "Trust re-decided: {} lost, {} gained.",
                changes.lost.len(),
                changes.gained.len()
            ),
            serde_json::json!({ "lost": changes.lost, "gained": changes.gained }),
        )
        .await;
        log::trust_reevaluated(instances.len(), changes.lost.len(), changes.gained.len());
        Ok(changes)
    }

    pub async fn executable_part_decisions(
        &self,
        id: AgentInstanceId,
    ) -> Result<Vec<ExecutablePartDecision>, CheckError> {
        Ok(store::load_decisions(self.pool()?, id)
            .await?
            .into_iter()
            .map(|r| r.decision)
            .collect())
    }

    pub async fn forget_instance(&self, id: AgentInstanceId) -> Result<(), CheckError> {
        store::forget(self.pool()?, id).await
    }

    pub async fn report(&self, id: CheckRunId) -> Result<CheckReport, CheckError> {
        store::load_run(self.pool()?, id)
            .await?
            .ok_or(CheckError::RunNotFound { id })
    }
}

/// Check a package's parts from `source` for the Release Builder (no datastore, no record, no
/// trust policy, no bound source). Sections C and D steps 9–12 over the root's closure.
pub async fn check_parts(
    source: &dyn PartSource,
    request: &ordin_resolver::ResolveRequest,
) -> Result<PackageCheckReport, CheckError> {
    let compiled = match ordin_resolver::compile_instructions(source, request).await {
        Ok(compiled) => compiled,
        Err(ResolveError::SourceUnavailable { retryable }) => {
            return Err(CheckError::SourceUnavailable { retryable })
        }
        Err(error) => {
            return Ok(PackageCheckReport {
                findings: vec![PackageFinding {
                    stop: true,
                    file: String::new(),
                    entry: error.code().to_owned(),
                    message: format!("the package does not resolve: {}", error.code()),
                }],
            })
        }
    };
    let lock = &compiled.lock;
    let mut named: Vec<PartRef> = vec![lock.composition.clone()];
    named.extend(lock.artefacts.iter().cloned());
    let mut findings = Vec::new();
    let items = read_named(source, &request.scope, &named, &mut findings).await?;
    let vocabulary = source
        .vocabulary(&request.scope)
        .await
        .map_err(from_source)?;
    // The file of each part holding its manifest, confirmed against the source's files.
    let mut files: Vec<String> = Vec::with_capacity(items.len());
    for item in &items {
        let path = item.path.clone();
        match &item.sourced {
            Some(sourced) => {
                let held = source.files(sourced).await.map_err(from_source)?;
                let holds = held.0.keys().any(|k| path.ends_with(k.as_str()));
                files.push(if holds { path } else { String::new() });
            }
            None => files.push(path),
        }
    }
    let file_of_id = |id: Option<&str>| -> String {
        id.and_then(|id| items.iter().position(|i| i.id() == id))
            .map(|at| files[at].clone())
            .unwrap_or_default()
    };
    let to_package = |f: &Finding, file: String| PackageFinding {
        stop: f.severity == Severity::Failure,
        file,
        entry: f.part_identifier.clone().unwrap_or_else(|| f.code.clone()),
        message: f.detail.clone(),
    };
    let mut out: Vec<PackageFinding> = findings
        .iter()
        .map(|f| to_package(f, file_of_id(f.part_identifier.as_deref())))
        .collect();
    for (at, item) in items.iter().enumerate() {
        let view = PartView {
            kind: item.kind,
            manifest: &item.manifest,
            body: item
                .files
                .as_ref()
                .and_then(|files| stage_one::body_of_files(&item.path, files)),
        };
        out.extend(
            stage_one::check(&view, &vocabulary)
                .iter()
                .map(|f| to_package(f, files[at].clone())),
        );
    }
    let lookup = Lookup {
        source,
        scope: &request.scope,
    };
    let refs: Vec<&Item> = items.iter().collect();
    let gates = gate_evaluations(source, &items).await?;
    let mut rest = cross_checks(&items, &gates);
    rest.extend(repo::references(&refs, &items, Some(&lookup)).await?);
    rest.extend(ingest_rules(&items, &vocabulary));
    out.extend(
        rest.iter()
            .map(|f| to_package(f, file_of_id(f.part_identifier.as_deref()))),
    );
    Ok(PackageCheckReport { findings: out })
}
