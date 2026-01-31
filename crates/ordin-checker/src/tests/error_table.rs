//! X1: one test per row of Checker.md *Error Table*. In the crate because several rows need the
//! failing source and the bound-resolver seams of `support`.

use std::sync::Arc;

use ordin_agent_model::{PartSource, PartSourceError, VocabularyAxis};
use ordin_types::domain::agents::ExecutablePartKind;
use ordin_types::AgentInstanceId;
use sqlx::ConnectOptions;

use super::acceptance::{clean_draft, contract, draft_check, withdrawal_entry};
use super::support::*;
use crate::context::resolve_request;
use crate::model::*;
use crate::CheckError;

const ROOT: &str = "urn:ordin:acme:workspaces:clause-checker";

/// A repository with one clean agent, and its lock file.
async fn agent_and_lock() -> (Arc<dyn PartSource>, ordin_agent_model::LockFile) {
    let mut home = base();
    home.push(workspace("clause-checker", &minimal_body()));
    let source: Arc<dyn PartSource> = source_of(home, Vec::new());
    let request = resolve_request(&scope(), ROOT, Vec::new(), Vec::new(), false, &source);
    let lock = ordin_resolver::compile_instructions(source.as_ref(), &request)
        .await
        .expect("the agent compiles")
        .lock;
    (source, lock)
}

fn read_subject() -> CheckSubject {
    CheckSubject::RepositoryRead {
        repository: ACME_LABEL.to_owned(),
        commit: "b".repeat(40),
    }
}

// Row: `PartSource` cannot read the clone or index.
#[tokio::test]
async fn source_cannot_read_the_clone() {
    let f = Fixture::with(Arc::new(FailingSource {
        inner: source_of(base(), Vec::new()),
        fails: Fails::RepositoryParts,
        error: PartSourceError::Unavailable { retryable: true },
    }))
    .await;
    let error = f
        .checker
        .check_repository(&repository_check(Vec::new()), read_subject())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        CheckError::SourceUnavailable { retryable: true }
    ));
    assert_eq!(error.code(), "CHK_SOURCE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(f.runs().await, 0);
}

// Row: `PartSource` reports an unreadable withdrawn list with no earlier copy.
#[tokio::test]
async fn withdrawn_list_unreadable() {
    let (source, lock) = agent_and_lock().await;
    let f = Fixture::unbound().await;
    f.resolver.install_part_source(Arc::clone(&source)).unwrap();
    f.checker
        .install_part_source(Arc::new(FailingSource {
            inner: source,
            fails: Fails::Withdrawn,
            error: PartSourceError::WithdrawnListUnreadable {
                repository: ACME_LABEL.to_owned(),
            },
        }))
        .unwrap();
    let error = f
        .checker
        .check_installed(AgentInstanceId::new(), &lock, &scope())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        CheckError::SourceUnavailable { retryable: false }
    ));
    assert!(!error.retryable());
    assert_eq!(f.runs().await, 0);
}

// Row: Resolver and Compiler fails for a reason that is not a finding (source, store).
#[tokio::test]
async fn resolver_fails_for_a_reason_that_is_not_a_finding() {
    let (source, lock) = agent_and_lock().await;
    // The resolver's source cannot be read: retryable as the source says.
    let f = Fixture::unbound().await;
    f.resolver
        .install_part_source(Arc::new(FailingSource {
            inner: Arc::clone(&source),
            fails: Fails::Resolve,
            error: PartSourceError::Unavailable { retryable: true },
        }))
        .unwrap();
    f.checker.install_part_source(Arc::clone(&source)).unwrap();
    let error = f
        .checker
        .check_installed(AgentInstanceId::new(), &lock, &scope())
        .await
        .unwrap_err();
    match &error {
        CheckError::ResolverFailed { code, retryable } => {
            assert_eq!(code, "RES_SOURCE_UNAVAILABLE");
            assert!(*retryable);
        }
        other => panic!("expected CHK_RESOLVER_FAILED, got {other:?}"),
    }
    // A resolver the program never bound: permanent.
    let f = Fixture::unbound().await;
    f.checker.install_part_source(source).unwrap();
    let error = f
        .checker
        .check_installed(AgentInstanceId::new(), &lock, &scope())
        .await
        .unwrap_err();
    match &error {
        CheckError::ResolverFailed { code, retryable } => {
            assert_eq!(code, "RES_SOURCE_NOT_BOUND");
            assert!(!*retryable);
        }
        other => panic!("expected CHK_RESOLVER_FAILED, got {other:?}"),
    }
    assert_eq!(error.code(), "CHK_RESOLVER_FAILED");
    assert_eq!(f.runs().await, 0);
}

// Row: Request malformed (job index outside `draft.jobs`, empty `entry.reason` with no list).
// The axis is a closed enum in the request type, so an unknown axis cannot be written.
#[tokio::test]
async fn request_malformed() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let mut check = draft_check(
        clean_draft(),
        vec![workspace("clause-checker", &minimal_body())],
    );
    check
        .job_tasks
        .insert(7, vec!["Compare clauses".to_owned()]);
    let error = f.checker.check_agent_draft(&check).await.unwrap_err();
    assert!(matches!(
        error,
        CheckError::InputInvalid { field: "job_tasks" }
    ));
    assert!(!error.retryable());

    let entry = withdrawal_entry("urn:ordin:acme:rules:tone", None, "");
    let check = WithdrawalCheck {
        repository: repository_check(Vec::new()),
        entry,
        existing_entries: Vec::new(),
        proposed_list: Vec::new(),
    };
    let error = f
        .checker
        .check_withdrawal(&check, ordin_types::PartDraftId::new())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        CheckError::InputInvalid {
            field: "entry.reason"
        }
    ));
    assert_eq!(error.code(), "CHK_INPUT_INVALID");
    assert_eq!(f.runs().await, 0);
    // The axis is typed: every axis a request can carry is a known one.
    let _: VocabularyAxis = VocabularyAxis::ArtefactStatus;
}

// Row: Subject unreadable even for stage one (no part at all in a draft).
#[tokio::test]
async fn draft_with_no_part_at_all() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let check = draft_check(clean_draft(), Vec::new());
    let error = f.checker.check_agent_draft(&check).await.unwrap_err();
    assert!(matches!(error, CheckError::TooManyFindings));
    assert_eq!(error.code(), "CHK_TOO_MANY_FINDINGS");
    assert!(!error.retryable());
    assert_eq!(f.runs().await, 0);
}

// Row: `report` for an unknown or pruned run.
#[tokio::test]
async fn report_of_an_unknown_run() {
    let f = Fixture::unbound().await;
    let id = CheckRunId(uuid::Uuid::now_v7());
    let error = f.checker.report(id).await.unwrap_err();
    assert!(matches!(error, CheckError::RunNotFound { id: got } if got == id));
    assert_eq!(error.code(), "CHK_RUN_NOT_FOUND");
    assert!(!error.retryable());
}

fn files_part() -> [ExecutablePart; 1] {
    [ExecutablePart {
        part_name: "files".to_owned(),
        kind: ExecutablePartKind::McpServer,
        declared_by: "urn:ordin:acme:tool-bindings:files".to_owned(),
        bundled: false,
    }]
}

// Row: Datastore busy or locked. Another connection holds the write lock past the datastore's busy
// timeout.
#[tokio::test]
async fn datastore_busy_or_locked() {
    let f = Fixture::unbound().await;
    let mut holder = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(f.db_path())
        .connect()
        .await
        .expect("a second connection");
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut holder)
        .await
        .expect("hold the write lock");
    let error = f
        .checker
        .decide_executable_parts(AgentInstanceId::new(), &files_part())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        CheckError::DatastoreFailed { retryable: true }
    ));
    assert!(error.retryable());
    sqlx::query("ROLLBACK").execute(&mut holder).await.unwrap();
}

// Row: Other datastore failure.
#[tokio::test]
async fn other_datastore_failure() {
    let f = Fixture::unbound().await;
    sqlx::query("DROP TABLE executable_part_decisions")
        .execute(f.pool())
        .await
        .expect("drop the table");
    let error = f
        .checker
        .decide_executable_parts(AgentInstanceId::new(), &files_part())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        CheckError::DatastoreFailed { retryable: false }
    ));
    assert_eq!(error.code(), "CHK_DATASTORE_FAILED");
    assert!(f.journal_kinds().await.is_empty());
}

// Row: A part-reading call before `install_part_source` and without a per-request source.
#[tokio::test]
async fn part_reading_call_before_the_source_is_bound() {
    let f = Fixture::unbound().await;
    let error = f
        .checker
        .check_library_item(&scope(), ordin_agent_model::PartKind::Rule, TONE_ID)
        .await
        .unwrap_err();
    assert!(matches!(error, CheckError::SourceNotBound));
    assert_eq!(error.code(), "CHK_SOURCE_NOT_BOUND");
    assert!(!error.retryable());
    assert_eq!(f.runs().await, 0);
    // The pure operations read no part and do not fail this way.
    assert!(f
        .checker
        .check_rehearsal(&[ExpectationRule::ApprovalAsked], &RunEvidence::default())
        .iter()
        .all(|o| !o.met));
}

// Row: `install_part_source` called a second time.
#[tokio::test]
async fn source_bound_a_second_time() {
    let f = Fixture::with(source_of(base(), Vec::new())).await;
    let error = f
        .checker
        .install_part_source(source_of(Vec::new(), Vec::new()))
        .unwrap_err();
    assert!(matches!(error, CheckError::SourceAlreadyBound));
    assert_eq!(error.code(), "CHK_SOURCE_ALREADY_BOUND");
    assert!(!error.retryable());
    // The first source stays in place: the base still reads.
    let report = f
        .checker
        .check_library_item(&scope(), ordin_agent_model::PartKind::Rule, TONE_ID)
        .await
        .unwrap();
    assert!(report.passed, "findings: {:?}", report.findings);
}

// Row: `executable_parts` for an instance with no install check stored.
#[tokio::test]
async fn executable_parts_with_no_install_check() {
    let f = Fixture::unbound().await;
    let id = AgentInstanceId::new();
    let error = f.checker.executable_parts(id).await.unwrap_err();
    assert!(matches!(error, CheckError::InstanceUnknown { id: got } if got == id));
    assert_eq!(error.code(), "CHK_INSTANCE_UNKNOWN");
    assert!(!error.retryable());
}

// Row: `read_dossier` given a part that is not an OutputContract, has no sections, or has a section
// without a name.
#[tokio::test]
async fn read_dossier_given_an_unusable_contract() {
    let f = Fixture::unbound().await;
    let cases = [
        (
            rule("tone", "Use short sentences.\n"),
            "not_output_contract",
        ),
        (contract("empty", "  []\n"), "no_sections"),
        (
            contract("unnamed", "  - meaning: What it does.\n"),
            "section_malformed",
        ),
    ];
    for (part, reason) in cases {
        let error = f.checker.read_dossier("# Purpose\n", &part).unwrap_err();
        assert!(
            matches!(error, CheckError::ContractInvalid { reason: got } if got == reason),
            "{reason}: got {error:?}"
        );
        assert_eq!(error.code(), "CHK_CONTRACT_INVALID");
        assert!(!error.retryable());
    }
}
