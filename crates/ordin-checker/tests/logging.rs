//! The log events of Checker.md *Observability*: each event's level and fields, and that no part
//! text reaches a line.
//!
//! Alone in its test binary (plan FU-017): `tracing` caches per call site whether anything listens,
//! so a scoped subscriber can be missed when another test thread logs first. Here one global
//! subscriber is installed before any test logs, and each test reads only the lines of its own
//! events or identifiers.
//!
//! `checker.trust_policy_invalid` is not reached from here: the Organisation Profile verifies the
//! trust policy before a profile becomes current, so through the public constructor the Checker only
//! ever reads a valid one or the empty default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ordin_agent_model::testkit::{ContractFixture, InMemorySource};
use ordin_agent_model::{
    read_tree, ModelPart, PartKind, PartSource, RepositoryRef, SourceScope, StagedPart,
    VocabularySet,
};
use ordin_checker::{
    Checker, ExecutablePart, HandoverCheckInput, HandoverResult, ReceiverFacts, RepositoryCheck,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_resolver::{ResolverAndCompiler, ResolverConfig};
use ordin_types::domain::agents::ExecutablePartKind;
use ordin_types::{AgentInstanceId, AgentRepositoryId, PartDraftId};
use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn buffer() -> &'static Buffer {
    static BUFFER: OnceLock<Buffer> = OnceLock::new();
    BUFFER.get_or_init(|| {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ordin_types::log::JsonLineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::set_global_default(subscriber).expect("one global subscriber");
        buffer.clone()
    })
}

/// Every line of the event `event` written so far.
fn events(event: &str) -> Vec<Value> {
    let text = String::from_utf8(buffer().0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("a JSON line"))
        .filter(|line| line["event"] == event)
        .collect()
}

/// The one line of `event` whose `field` is `value`.
fn event_with(event: &str, field: &str, value: &str) -> Value {
    let lines: Vec<Value> = events(event)
        .into_iter()
        .filter(|l| l[field] == value)
        .collect();
    assert_eq!(lines.len(), 1, "{event} with {field} = {value}: {lines:?}");
    let line = lines.into_iter().next().unwrap();
    assert!(line.get("dropped_fields").is_none(), "{line}");
    line
}

const TONE_TEXT: &str = "Use short sentences about the Meridian contract.";

fn part(path: &str, text: &str) -> ModelPart {
    let mut files = BTreeMap::new();
    files.insert(path.to_owned(), text.as_bytes().to_vec());
    let reading = read_tree(&files, &VocabularySet::default());
    assert!(reading.refused.is_empty(), "{:?}", reading.refused);
    reading.parts.into_iter().next().expect("one part")
}

fn rule(slug: &str, extra_metadata: &str, text: &str) -> ModelPart {
    part(
        &format!("models/@acme/rules/{slug}.md"),
        &format!(
            "---\napiVersion: ai.enterprise/v1\nkind: rule\nmetadata:\n  id: urn:ordin:acme:rules:{slug}\n  name: {slug}\n  version: 1.0.0\n  owner: legal-ops\n{extra_metadata}---\n{text}\n"
        ),
    )
}

fn acme() -> RepositoryRef {
    RepositoryRef::Repository(AgentRepositoryId(uuid::uuid!(
        "01900000-0000-7000-8000-0000000000a1"
    )))
}

fn scope() -> SourceScope {
    SourceScope {
        home: acme(),
        profile: Vec::new(),
        include_bundled: false,
    }
}

fn source_of(parts: Vec<ModelPart>) -> Arc<dyn PartSource> {
    Arc::new(InMemorySource::new(&ContractFixture {
        repositories: vec![(acme(), "github.com/acme/agents".to_owned(), parts)],
        withdrawn: Vec::new(),
        refused: Vec::new(),
    }))
}

struct Now;

impl Clock for Now {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Checker over a fresh SQLite journal; bound to `source` when one is given.
async fn checker(source: Option<Arc<dyn PartSource>>) -> (Checker, Temp) {
    buffer();
    let temp =
        Temp(std::env::temp_dir().join(format!("ordin-checker-log-{}", uuid::Uuid::now_v7())));
    let dir: &Path = &temp.0;
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.join("journal.db"),
        backup_dir: dir.join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(ordin_checker::migrations()).unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let journal = Arc::new(
        Journal::start(
            store,
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .unwrap(),
    );
    let resolver = Arc::new(
        ResolverAndCompiler::new(ResolverConfig {
            bundle_root: dir.join("bundles"),
            compiler_version: "0.1.0-test".to_owned(),
        })
        .unwrap(),
    );
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(Now),
            },
        )
        .await
        .unwrap(),
    );
    if let Some(source) = &source {
        resolver.install_part_source(Arc::clone(source)).unwrap();
    }
    let checker = Checker::new(resolver, profile, journal);
    if let Some(source) = source {
        checker.install_part_source(source).unwrap();
    }
    (checker, temp)
}

#[tokio::test]
async fn check_run_carries_counts_and_never_part_text() {
    let (checker, _temp) = checker(Some(source_of(vec![rule("tone", "", TONE_TEXT)]))).await;
    let report = checker
        .check_library_item(&scope(), PartKind::Rule, "urn:ordin:acme:rules:tone")
        .await
        .unwrap();
    let line = event_with(
        "checker.check_run",
        "check_run_id",
        &report.check_run_id.to_string(),
    );
    assert_eq!(line["level"], "INFO");
    assert_eq!(line["subject_kind"], "library_item");
    assert_eq!(line["passed"], true);
    assert_eq!(line["failures"], 0);
    assert_eq!(line["warnings"], 0);
    assert_eq!(line["tidied"], 0);
    assert_eq!(line["parts"], 1);
    assert!(line["duration_ms"].is_u64(), "{line}");
    assert!(!line.to_string().contains("Meridian"), "{line}");
}

#[tokio::test]
async fn an_unbound_source_logs_source_not_bound_and_check_failed() {
    let (checker, _temp) = checker(None).await;
    checker
        .check_library_item(&scope(), PartKind::Rule, "urn:ordin:acme:rules:tone")
        .await
        .unwrap_err();
    let line = event_with(
        "checker.source_not_bound",
        "operation",
        "check_library_item",
    );
    assert_eq!(line["level"], "ERROR");
    let failed: Vec<Value> = events("checker.check_failed")
        .into_iter()
        .filter(|l| l["error_code"] == "CHK_SOURCE_NOT_BOUND")
        .collect();
    assert!(!failed.is_empty());
    assert_eq!(failed[0]["level"], "WARNING");
    assert_eq!(failed[0]["subject_kind"], "library_item");
    assert_eq!(failed[0]["retryable"], false);
}

#[tokio::test]
async fn trust_decisions_and_reevaluation_are_logged() {
    let (checker, _temp) = checker(None).await;
    let instance = AgentInstanceId::new();
    let parts = [ExecutablePart {
        part_name: "files".to_owned(),
        kind: ExecutablePartKind::McpServer,
        declared_by: "urn:ordin:acme:tool-bindings:files".to_owned(),
        bundled: false,
    }];
    checker
        .decide_executable_parts(instance, &parts)
        .await
        .unwrap();
    let line = event_with(
        "checker.executable_parts_decided",
        "agent_instance_id",
        &instance.to_string(),
    );
    assert_eq!(line["level"], "INFO");
    assert_eq!(line["trusted"], 0);
    assert_eq!(line["needs_it_approval"], 1);
    assert!(
        !line.to_string().contains("files\""),
        "no part names: {line}"
    );

    checker.reevaluate_trust().await.unwrap();
    let lines = events("checker.trust_reevaluated");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "INFO");
    assert_eq!(lines[0]["instances"], 1);
    assert_eq!(lines[0]["lost"], 0);
    assert_eq!(lines[0]["gained"], 0);
}

#[tokio::test]
async fn handover_checked_names_the_failed_codes() {
    let (checker, _temp) = checker(None).await;
    let report = checker
        .check_handover(&HandoverCheckInput {
            results: vec![HandoverResult {
                link: "file:///Users/asha/meridian-notes.md".to_owned(),
                label: "Meridian notes".to_owned(),
                local_only: true,
                readable_by_receiver: None,
                headings: None,
            }],
            contract: None,
            receiver: ReceiverFacts {
                is_person: true,
                reachable: true,
            },
        })
        .await
        .unwrap();
    assert!(!report.passed);
    let lines = events("checker.handover_checked");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["level"], "INFO");
    assert_eq!(lines[0]["passed"], false);
    assert_eq!(lines[0]["failed_codes"], "results_readable");
    assert!(!lines[0].to_string().contains("meridian"), "{}", lines[0]);
}

#[tokio::test]
async fn a_finding_already_in_the_repository_is_logged_at_debug() {
    let stale = rule(
        "stale-tone",
        "  supersedes: urn:ordin:acme:rules:gone\n",
        "Old wording.",
    );
    let (checker, _temp) = checker(Some(source_of(vec![rule("tone", "", TONE_TEXT), stale]))).await;
    let change = StagedPart {
        part: rule("tone", "", "Use shorter sentences."),
        repository: acme(),
        repository_label: "github.com/acme/agents".to_owned(),
    };
    let report = checker
        .check_part_draft(
            &RepositoryCheck {
                repository: acme(),
                repository_display_name: "Acme agents".to_owned(),
                scope: scope(),
                changes: vec![change],
                deletions: Vec::new(),
                source: None,
            },
            PartDraftId::new(),
            &BTreeMap::new(),
        )
        .await
        .unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| f.code != "sync_unknown_supersedes_target"),
        "pre-existing findings are not reported: {:?}",
        report.findings
    );
    let line = event_with(
        "checker.preexisting_finding",
        "part_identifier",
        "urn:ordin:acme:rules:stale-tone",
    );
    assert_eq!(line["level"], "DEBUG");
    assert_eq!(line["code"], "sync_unknown_supersedes_target");
}
