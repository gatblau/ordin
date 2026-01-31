//! The update check's candidate inspection (§C3, §C5) against a real Package Inspector: the
//! candidate is inspected in its own bundle folder, whose `ordin.lock` names the candidate's plan,
//! never in the installed folder, whose `ordin.lock` names the installed plan. The installed
//! inspection after the update (§A8) stays the double's; it is the installed folder's (asserted
//! through the request it receives).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_catalogue::PartStateLabel;
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_package_inspector::{may_do, InspectRequest, PackageInspector};
use ordin_resolver::model::{
    PartKind, ResolutionPlan, WithdrawnListEntry, BUNDLED_REPOSITORY_ID, BUNDLE_MANIFEST_NAME,
    LOCK_FILE_NAME,
};
use ordin_resolver::ResolveError;
use ordin_rules::{
    CardWording, ContentChecker, ControlLine, FactError, MessageRule, ProgramKind, Proposal,
    RulesConfig, RulesEngine, RulesHost, RunKey,
};
use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, PartId, RunId, WithdrawnListEntryId};

use super::support::*;
use crate::types::{AgentNotice, BundledAgent};

const COORDINATOR: &str = "urn:ordin:default:workspaces:coordinator";

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// The Rules Engine's host; the candidate inspection never calls the engine (step 6a is skipped).
struct Host;

#[async_trait::async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _run: &RunKey, _path: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(
        &self,
        _run: &RunKey,
        _path: &str,
        _text: &str,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _run: &RunKey, _path: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _run: &RunKey, _path: &str, _line: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _run: &RunKey, _path: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn runs_in_progress(&self, _command: &str) -> Result<u32, FactError> {
        Ok(0)
    }
    async fn paused_run_with_command(
        &self,
        _commands: &[String],
        _except: RunId,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _run_id: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(
        &self,
        _scope: Option<&str>,
        _name: &str,
    ) -> Result<serde_json::Value, FactError> {
        Ok(serde_json::Value::Null)
    }
    async fn setting_rules(&self, _scope: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _run: &RunKey, _proposal: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "The run's work".to_owned(),
        }];
        CardWording {
            question: "Allow this action?".to_owned(),
            verb: "Allow".to_owned(),
            where_line: "This computer".to_owned(),
            affects: affects.clone(),
            changes: "The action runs.".to_owned(),
            content_label: None,
            proposal: ActionProposal {
                action: "Run a tool".to_owned(),
                destination: "This computer".to_owned(),
                affects,
                consequence: "The action runs.".to_owned(),
            },
        }
    }
    fn plain_line(&self, line: &ControlLine) -> String {
        format!("{} applies.", line.name)
    }
    fn content_checker(&self) -> Option<Arc<dyn ContentChecker>> {
        None
    }
}

/// A real Package Inspector over a real Rules Engine on its own SQLite datastore under `root`.
async fn real_inspector(root: &Path) -> Arc<PackageInspector> {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: root.join("rules").join("journal.db"),
        backup_dir: root.join("rules").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ApprovalService::migrations())
        .expect("approval");
    registry.register(RulesEngine::migrations()).expect("rules");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal"),
    );
    let approvals = ApprovalService::new(
        store.clone(),
        journal.clone(),
        ApprovalConfig {
            backend: BackendId(journal.backend_uuid()),
            local_computer_name: "This computer".to_owned(),
            approvers: None,
        },
    )
    .await
    .expect("approval service");
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(WallClock),
            },
        )
        .await
        .expect("profile service"),
    );
    let engine = RulesEngine::new(
        store,
        journal,
        profile,
        approvals,
        Arc::new(Host),
        RulesConfig {
            ordin_servers: ["ordin", "work"].iter().map(|s| (*s).to_owned()).collect(),
            ordin_server_prefixes: Vec::new(),
            protected_roots: Vec::new(),
            app_data_root: root.join("app"),
            program: ProgramKind::Desktop,
            instance_name: "local".to_owned(),
        },
    );
    Arc::new(PackageInspector::new(engine, root.join("inspector")).expect("the inspector"))
}

/// Writes the bundle the Resolver and Compiler would write for `plan`: `ordin.lock` and, when
/// `with_manifest`, `bundle.manifest.json`. Returns its folder.
fn write_bundle(fx: &Fx, plan: &ResolutionPlan, with_manifest: bool) -> PathBuf {
    let bundle = fx.resolver.bundle_of(plan.clone());
    std::fs::create_dir_all(&bundle.dir).expect("the bundle folder");
    std::fs::write(
        bundle.dir.join(LOCK_FILE_NAME),
        bundle.lock_file.to_canonical_yaml(),
    )
    .expect("ordin.lock");
    if with_manifest {
        std::fs::write(
            bundle.dir.join(BUNDLE_MANIFEST_NAME),
            serde_json::to_vec(&bundle.manifest).expect("the manifest"),
        )
        .expect("bundle.manifest.json");
    }
    bundle.dir
}

/// Gives the installed instance a real folder holding the installed plan's `ordin.lock`, as the
/// Installers leave it; the candidate inspection must not read it.
async fn installed_folder(fx: &Fx, id: AgentInstanceId, installed: &ResolutionPlan) -> PathBuf {
    let folder = fx.dir.0.join("agents").join(id.to_string()).join("folder");
    std::fs::create_dir_all(&folder).expect("the agent folder");
    let lock = fx.resolver.bundle_of(installed.clone()).lock_file;
    std::fs::write(folder.join(LOCK_FILE_NAME), lock.to_canonical_yaml()).expect("ordin.lock");
    sqlx::query("UPDATE agent_instances SET agent_folder_path = ?1 WHERE id = ?2")
        .bind(folder.to_string_lossy().into_owned())
        .bind(id.to_string())
        .execute(&fx.pool)
        .await
        .expect("update the folder path");
    folder
}

/// Makes the double's installed `may_do` what the real inspector reads from `bundle_dir`, so an
/// unchanged candidate is a silent update (§C3).
async fn same_may_do_as(fx: &Fx, real: &PackageInspector, bundle_dir: &Path) {
    let model = real
        .inspect_candidate(&InspectRequest {
            agent_instance_id: AgentInstanceId::new(),
            folder_path: bundle_dir.to_path_buf(),
            bundle_dir: Some(bundle_dir.to_path_buf()),
            repository_names: Default::default(),
            part_states: Default::default(),
            install: None,
        })
        .await
        .expect("the bundle inspects");
    *lock(&fx.inspector.may_do) = may_do(&model);
}

/// An installed agent at plan A with plan B at the head, a real candidate inspector, and a real
/// installed folder at plan A. Returns the fixture, the instance and the two folders.
async fn installed_at_a_with_b_at_the_head() -> (Fx, AgentInstanceId, PathBuf, PathBuf) {
    let fx = Fx::new().await;
    let plan_a = plan("a1", COMMIT_A, "1.2.0");
    let plan_b = plan("b1", COMMIT_B, "1.3.0");
    let real = real_inspector(&fx.dir.0).await;
    let bundle_b = write_bundle(&fx, &plan_b, true);
    same_may_do_as(&fx, &real, &bundle_b).await;
    *lock(&fx.inspector.real_candidate) = Some(real);
    let id = fx.add().await.instance.id;
    let folder = installed_folder(&fx, id, &plan_a).await;
    fx.set_plan(plan_b);
    lock(&fx.inspector.requests).clear();
    (fx, id, folder, bundle_b)
}

/// The candidate was inspected in its bundle folder and the update re-inspected the installed one.
fn assert_inspected_in_place(fx: &Fx, folder: &Path, bundle_b: &Path) {
    let requests = lock(&fx.inspector.requests).clone();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert_eq!(requests[0].folder_path, bundle_b, "the candidate's folder");
    assert_eq!(requests[0].bundle_dir.as_deref(), Some(bundle_b));
    assert_eq!(requests[1].folder_path, folder, "the installed folder");
    assert_eq!(requests[1].bundle_dir.as_deref(), Some(bundle_b));
}

// Scenario: an installed agent at plan A, with plan B at the head, is updated by `run`'s start
// sweep (§C3).
// spec: ordin-desktop/components/PackageCatalogue#SA-RECON-07
#[tokio::test]
async fn the_start_sweep_inspects_the_candidate_in_its_bundle_and_updates_the_agent() {
    let (fx, id, folder, bundle_b) = installed_at_a_with_b_at_the_head().await;
    let stop = shutdown();
    let pc = fx.pc.clone();
    let token = stop.clone();
    let running = tokio::spawn(async move { pc.run(token).await });
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.package_version == "1.3.0")
    })
    .await;
    stop.cancel();
    running
        .await
        .expect("the loop ends")
        .expect("the loop ends without an error");
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(updated.instance.status, AgentInstanceStatus::Ready);
    assert!(updated.instance.updated_between_runs_at.is_some());
    assert!(updated.update.is_none(), "nothing waits for the person");
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
    assert_eq!(
        updated.instance.agent_folder_path,
        folder.to_string_lossy(),
        "the update installs into the instance's own folder"
    );
    assert_inspected_in_place(&fx, &folder, &bundle_b);
}

// Scenario: the same agent is updated on a `RepositoryUpdated` notice (§C3).
#[tokio::test]
async fn a_repository_notice_inspects_the_candidate_in_its_bundle_and_updates_the_agent() {
    let (fx, id, folder, bundle_b) = installed_at_a_with_b_at_the_head().await;
    let mut notices = fx.pc.subscribe();
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(updated.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(updated.instance.package_version, "1.3.0");
    assert!(updated.update.is_none(), "nothing waits for the person");
    assert!(matches!(
        notices.try_recv().expect("a notice"),
        AgentNotice::UpdatedBetweenRuns { agent_instance_id } if agent_instance_id == id
    ));
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
    assert_inspected_in_place(&fx, &folder, &bundle_b);
}

// Scenario: the bundled Ordin agent is updated without asking when the application brings a new
// bundled version (PKG-REC-3, §C3 start sweep).
// spec: ordin-desktop/components/PackageCatalogue#PKG-REC-3
#[tokio::test]
async fn the_bundled_ordin_agent_takes_a_new_bundled_version_silently() {
    let fx = Fx::new().await;
    lock(&fx.catalogue.parts).push(view(
        part(
            PartId::new(),
            BUNDLED_REPOSITORY_ID,
            PartKind::WorkspaceProfile,
            COORDINATOR,
            "Ordin",
            "0.1.0",
        ),
        true,
        PartStateLabel::Current,
    ));
    let plan_a = plan("a1", COMMIT_A, "0.1.0");
    let plan_b = plan("b1", COMMIT_B, "0.1.1");
    fx.set_plan(plan_a.clone());
    *lock(&fx.inspector.real_candidate) = Some(real_inspector(&fx.dir.0).await);
    let id = BundledAgent::Ordin.instance_id();
    assert_eq!(id.to_string(), "01900000-0000-7000-8000-000000000101");
    fx.pc
        .bundled_instance(BundledAgent::Ordin, "en-GB")
        .await
        .expect("added");
    let folder = installed_folder(&fx, id, &plan_a).await;
    // The new bundled version changes what Ordin may do; a bundled agent is still never asked.
    *lock(&fx.inspector.may_do) = may_do_of(&["files"]);
    let bundle_b = write_bundle(&fx, &plan_b, true);
    fx.set_plan(plan_b);
    lock(&fx.inspector.requests).clear();
    let stop = shutdown();
    let pc = fx.pc.clone();
    let token = stop.clone();
    let running = tokio::spawn(async move { pc.run(token).await });
    fx.wait_until(|| async {
        fx.pc
            .get(id)
            .await
            .is_ok_and(|a| a.instance.package_version == "0.1.1")
    })
    .await;
    stop.cancel();
    running
        .await
        .expect("the loop ends")
        .expect("the loop ends without an error");
    let updated = fx.pc.get(id).await.expect("the instance");
    assert_eq!(updated.instance.status, AgentInstanceStatus::Ready);
    assert!(updated.update.is_none(), "a bundled agent is never asked");
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
    assert_inspected_in_place(&fx, &folder, &bundle_b);
}

// Error row (PackageInspector *Error Table*, `PKI_BUNDLE_UNREADABLE`, reached from §C3): a candidate
// bundle without `bundle.manifest.json` is unavailable; the installed version stays and nothing is
// installed.
#[tokio::test]
async fn an_unreadable_candidate_bundle_keeps_the_installed_version() {
    let fx = Fx::new().await;
    let plan_a = plan("a1", COMMIT_A, "1.2.0");
    let plan_b = plan("b1", COMMIT_B, "1.3.0");
    *lock(&fx.inspector.real_candidate) = Some(real_inspector(&fx.dir.0).await);
    let id = fx.add().await.instance.id;
    installed_folder(&fx, id, &plan_a).await;
    write_bundle(&fx, &plan_b, false);
    fx.set_plan(plan_b);
    fx.clear_log();
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let kept = fx.pc.get(id).await.expect("the instance");
    assert_eq!(kept.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(kept.instance.package_version, "1.2.0");
    assert!(kept.update.is_none(), "nothing waits for the person");
    assert_eq!(count(&fx.log_lines(), "inspector.inspect_candidate"), 1);
    assert_eq!(count(&fx.log_lines(), "installers.update"), 0);
}

// Scenario: a fixed version for a withdrawn agent is inspected in its bundle folder (§C5).
#[tokio::test]
async fn a_fixed_version_is_inspected_in_its_bundle_folder() {
    let (fx, id, folder, bundle_b) = installed_at_a_with_b_at_the_head().await;
    // Withdraw the pack the installed plan uses, with no replacement: the agent stops.
    let entry = WithdrawnListEntry {
        id: WithdrawnListEntryId::new(),
        agent_repository_id: fx.repo,
        part_kind: PartKind::PracticePack,
        part_identifier: GO_PACK.to_owned(),
        part_version: None,
        part_content_digest: None,
        reason: "unsafe".to_owned(),
        withdrawn_at: Timestamp::now(),
        replacement_identifier: None,
        replacement_version: None,
        removed_by_hand_seen_at: None,
        read_at: Timestamp::now(),
    };
    lock(&fx.catalogue.withdrawn).push(entry.clone());
    *lock(&fx.resolver.build_error) = Some(ResolveError::PartWithdrawn {
        part: GO_PACK.to_owned(),
        replacement: None,
    });
    fx.pc
        .on_repository_updated(fx.repo, &[entry.id], &shutdown())
        .await;
    assert_eq!(
        fx.pc.get(id).await.expect("the instance").instance.status,
        AgentInstanceStatus::Refused
    );
    // Plan B at the head passes its checks: an update waits and the status stays refused.
    *lock(&fx.resolver.build_error) = None;
    lock(&fx.inspector.requests).clear();
    fx.pc.on_repository_updated(fx.repo, &[], &shutdown()).await;
    let waiting = fx.pc.get(id).await.expect("the instance");
    assert_eq!(waiting.instance.status, AgentInstanceStatus::Refused);
    assert_eq!(waiting.update.expect("an update waits").to_commit, COMMIT_B);
    let requests = lock(&fx.inspector.requests).clone();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert_eq!(requests[0].folder_path, bundle_b);
    // Accepting it installs into the instance's own folder and re-inspects that folder.
    let accepted = fx.pc.accept_update(id).await.expect("accepted");
    assert_eq!(accepted.instance.status, AgentInstanceStatus::Ready);
    assert_eq!(accepted.instance.package_version, "1.3.0");
    let last = lock(&fx.inspector.requests)
        .last()
        .cloned()
        .expect("a request");
    assert_eq!(last.folder_path, folder);
}

fn may_do_of(connections: &[&str]) -> ordin_package_inspector::MayDo {
    super::support::may_do(connections)
}
