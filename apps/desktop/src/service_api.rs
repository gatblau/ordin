//! `DesktopServiceApi` (row 3.48) and `DesktopErrorSentences` (OrdinDesktop.md *Commands this program
//! answers itself*, *Internal Logic* step 5, DSK-UP-7).
//!
//! The trait has one method per catalogue row (`ordin_service_api::for_each_command!`); the methods
//! are generated here, and each one's body comes from [`answer!`]: a row written by hand is named
//! there, a row delegated as the catalogue states is an arm of `delegate!` (`service_api_rows.rs`),
//! and every other row answers `SVC_NOT_IN_THIS_VERSION`. A command of a group absent
//! from [`crate::groups::groups`] is refused by the Service API before it reaches this program, so
//! that answer is reached only by a row whose owning module this program does not answer yet.
//!
//! A module's restated input and result types (ServiceApi.md SA-RECON-02) have the owner's serde form,
//! so a delegation passes them through [`restate`].
//
// The program's own rows answered here over the window shell ([`Shell`]): `app.quit`,
// `app.pick_folder`, `settings.get` and `settings.change` of `preferences.json` ([`Preferences`]),
// `files.save_export`, `app.open_slack_link` and `app.open_web_page`. The four rows of memory's models
// (`app.memory_models`, `app.download_memory_models`, `app.stop_memory_models_download`,
// `app.decline_memory_models`) are answered over `crate::memory_models` (DSK-MD-12).
// `app.agent_settings` and `app.change_agent_setting` are answered over `preferences.json` and
// `crate::agent_settings` (DSK-AGM-1). `dashboards.export` and `dashboards.add_to_presentation` are
// answered in `service_api/dashboards.rs` over the Journal and `crate::dashboard_export` (DSK-DBX-1).
// `knowledge.export` is answered in `service_api/knowledge_export.rs` over the Knowledge Service's
// `get_page` and `crate::dashboard_export::render_page` (DSK-KEX-1).
//
// `settings.get` and `settings.change` (DSK-REC-37): `notifications`, `display` and `about` from
// `preferences.json`; every other section answers no settings, and any other key is refused with
// `SVC_INPUT_INVALID` (DSK-REC-48: the refusal carries no `at` in R1).
//
// `files.read_version`, `files.download`, `files.save_copy`, `diagnostics.support_export` and
// `workspaces.tree` are answered in `service_api_screens.rs` (DSK-REC-40 to DSK-REC-43).
//
// Rows not answered yet, each refused with `SVC_NOT_IN_THIS_VERSION` (never a stand-in answer); the
// reason of each is ServiceApi.md SVC-R1-3 (decisions D-1 to D-5):
// - `identity.relay_sign_in` and `identity.complete_relayed_sign_in`: they wait for the build with
//   a paired server (ServiceApi.md SVC-R1-4).
//
// `agents.installed_files`, `agents.part_page`, `checker.check_installed` and
// `checker.check_library_item` are answered in `service_api_owners.rs` (SVC-R1-2); the other rows
// of SVC-R1-1 are arms of `delegate!`.

use std::sync::Arc;

use ordin_approval::{ApprovalService, PressOrigin};
use ordin_brand::Brand;
use ordin_catalogue::CatalogueService;
use ordin_checker::Checker;
use ordin_confluence::ConfluenceAdapter;
use ordin_craft_toolkit::CraftToolkit;
use ordin_crew_orchestrator::CrewOrchestrator;
use ordin_files::FileService;
use ordin_home_search::HomeSearch;
use ordin_host::OrdinHost;
use ordin_identity::IdentityService;
use ordin_journal::Journal;
use ordin_knowledge::KnowledgeService;
use ordin_localisation::{Localisation, MessageArgs};
use ordin_memory_service::{MemoryPlace, MemoryService};
use ordin_organisation_profile::OrganisationProfileService;
use ordin_package_catalogue::PackageCatalogue;
use ordin_package_inspector::PackageInspector;
use ordin_projects::ProjectService;
use ordin_push::PushService;
use ordin_server_connector::ServerConnector;
use ordin_service_api::commands::{BackendProfile, ServiceHello};
use ordin_service_api::{
    CallContext, CallOrigin, ErrorSentences, ServiceApi, ServiceApiVersion, ServiceError,
};
use ordin_session_supervisor::SessionSupervisor;
use ordin_setup::{SetupAssistant, UrlOpener};
use ordin_types::{BackendId, CodedError, RepairAction, ScopeId, WorkspaceId};
use ordin_usage_planner::UsagePlanner;
use ordin_workspaces::WorkspaceService;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{DIALOG_CANCELLED, LINK_REFUSED, PICKER_UNAVAILABLE, SAVE_COPY_FAILED};
use crate::prefs::Preferences;
use crate::shell::{QuitTrigger, Shell};

/// The code of a row whose owning module this program does not answer.
pub const NOT_IN_THIS_VERSION: &str = "SVC_NOT_IN_THIS_VERSION";
/// A restated value whose form differs from its owner's.
const INTERNAL: &str = "SVC_INTERNAL";
/// An input the program's own checks refuse (`files.save_export`'s `file_name`).
const INPUT_INVALID: &str = "SVC_INPUT_INVALID";
/// The operating system refused to open a checked link (DSK-REC-38).
const BROWSER_FAILED: &str = "CRED_BROWSER_FAILED";

/// The Service API's `ErrorSentences` over Localisation (SA-RECON-07).
pub struct DesktopErrorSentences {
    pub localisation: Arc<Localisation>,
}

impl ErrorSentences for DesktopErrorSentences {
    fn sentence(&self, code: &str) -> String {
        self.localisation.error_sentence(code, &MessageArgs::new())
    }

    /// The action of a module error is its `CodedError::repair()`.
    fn action(&self, _code: &str) -> Option<RepairAction> {
        None
    }
}

/// The modules this program's commands reach; `None` for one this build has not constructed.
#[derive(Default, Clone)]
pub struct ApiModules {
    pub backend: Option<BackendId>,
    pub journal: Option<Arc<Journal>>,
    pub approval: Option<Arc<ApprovalService>>,
    pub localisation: Option<Arc<Localisation>>,
    pub catalogue: Option<Arc<CatalogueService>>,
    pub checker: Option<Arc<Checker>>,
    pub confluence: Option<Arc<ConfluenceAdapter>>,
    pub connector: Option<Arc<ServerConnector>>,
    pub craft: Option<Arc<CraftToolkit>>,
    pub files: Option<Arc<FileService>>,
    pub home: Option<Arc<HomeSearch>>,
    /// The `JournalReader` composite of row 3.42, the source of `workspaces.tree`'s running items
    /// (OrdinDesktop.md DSK-REC-45).
    pub journal_reader: Option<Arc<dyn ordin_home_search::JournalReader>>,
    pub identity: Option<Arc<IdentityService>>,
    pub inspector: Option<Arc<PackageInspector>>,
    pub knowledge: Option<Arc<KnowledgeService>>,
    pub memory: Option<Arc<MemoryService>>,
    /// Memory's models on this computer (DSK-MD-3 to DSK-MD-12).
    pub memory_models: Option<crate::memory_models::MemoryModels>,
    pub orchestrator: Option<Arc<CrewOrchestrator>>,
    pub ordin_host: Option<Arc<OrdinHost>>,
    pub packages: Option<Arc<PackageCatalogue>>,
    pub profile: Option<Arc<OrganisationProfileService>>,
    pub projects: Option<Arc<ProjectService>>,
    pub push: Option<Arc<PushService>>,
    pub setup: Option<Arc<SetupAssistant>>,
    pub installers: Option<Arc<ordin_installers::Installers>>,
    pub resolver: Option<Arc<ordin_resolver::ResolverAndCompiler>>,
    pub sharepoint: Option<Arc<ordin_sharepoint::SharePointAdapter>>,
    pub jira: Option<Arc<ordin_jira::JiraAdapter>>,
    pub supervisor: Option<Arc<SessionSupervisor>>,
    pub usage: Option<Arc<UsagePlanner>>,
    pub workspaces: Option<Arc<WorkspaceService>>,
    pub brand: Option<Arc<Brand>>,
    /// The window shell: dialogs, quit and the appearance event. `None` without a window.
    pub shell: Option<Arc<dyn Shell>>,
    /// `<data root>/preferences.json`.
    pub preferences: Option<Arc<Preferences>>,
    /// The harness agents run with since the program started (DSK-AGM-1); `None` reads it from the
    /// preferences and the profile.
    pub harness_in_use: Option<ordin_organisation_profile::HarnessName>,
    /// `UrlOpener` over `tauri-plugin-opener` (`app.open_slack_link`).
    pub url_opener: Option<Arc<dyn UrlOpener>>,
    /// Row 3.41a, behind `team-knowledge` (DSK-TW-3).
    #[cfg(feature = "team-knowledge")]
    pub team_knowledge: Option<Arc<ordin_team_knowledge::TeamKnowledgeService>>,
    /// Row 3.41a, behind `minutes` (DSK-MIN-1).
    #[cfg(feature = "minutes")]
    pub minutes: Option<Arc<ordin_minutes::MinutesService>>,
}

pub struct DesktopServiceApi {
    modules: ApiModules,
    sentences: Arc<dyn ErrorSentences>,
}

impl DesktopServiceApi {
    pub fn new(modules: ApiModules, sentences: Arc<dyn ErrorSentences>) -> DesktopServiceApi {
        DesktopServiceApi { modules, sentences }
    }

    /// A `ServiceError` for `code`, with its sentence and action (ServiceApi *Error mapping*).
    fn refuse(&self, code: &str, repair: Option<RepairAction>) -> ServiceError {
        ServiceError {
            code: code.to_owned(),
            message: self.sentences.sentence(code),
            action: repair.or_else(|| self.sentences.action(code)),
            reference: None,
        }
    }

    /// A module error `e` → `ServiceError { code: e.code(), message, action: e.repair() or … }`.
    fn module_error<E: CodedError>(&self, error: E) -> ServiceError {
        self.refuse(error.code(), error.repair())
    }

    fn not_answered(&self) -> ServiceError {
        self.refuse(NOT_IN_THIS_VERSION, None)
    }

    fn handle<T: Clone>(&self, slot: &Option<T>) -> Result<T, ServiceError> {
        slot.clone().ok_or_else(|| self.not_answered())
    }

    /// One value in the serde form of another (a module type and its restated twin).
    fn restate<A: Serialize, B: DeserializeOwned>(&self, value: A) -> Result<B, ServiceError> {
        serde_json::to_value(value)
            .and_then(serde_json::from_value)
            .map_err(|_| self.refuse(INTERNAL, None))
    }

    /// `desktop-backend-name` in the active locale; the en-GB text without Localisation.
    fn backend_name(&self) -> String {
        match &self.modules.localisation {
            Some(localisation) => localisation.message(
                ordin_localisation::id::DESKTOP_BACKEND_NAME,
                &MessageArgs::new(),
            ),
            None => "This computer".to_owned(),
        }
    }

    /// A *place* input: exactly one of `workspace_id` and `scope_id` (MemoryService.md, *Service API
    /// commands*; the input check has already refused any other combination).
    fn memory_place(
        &self,
        workspace_id: Option<WorkspaceId>,
        scope_id: Option<ScopeId>,
    ) -> Result<MemoryPlace, ServiceError> {
        match (workspace_id, scope_id) {
            (Some(workspace_id), None) => Ok(MemoryPlace::Workspace(workspace_id)),
            (None, Some(scope_id)) => Ok(MemoryPlace::Scope(scope_id)),
            _ => Err(self.refuse(INTERNAL, None)),
        }
    }

    /// One value in its serde form, for a result the owner returns unwrapped.
    fn value<A: Serialize>(&self, value: A) -> Result<serde_json::Value, ServiceError> {
        serde_json::to_value(value).map_err(|_| self.refuse(INTERNAL, None))
    }

    /// A refusal of this program's own code, with `try_again` when the Error Table marks it
    /// retryable.
    fn own_refusal(&self, code: &str, retryable: bool) -> ServiceError {
        self.refuse(code, retryable.then_some(RepairAction::TryAgain))
    }

    /// `app.quit`: the orderly shutdown of step 10, started without asking; the reply carries the
    /// number of local sessions it stops.
    fn app_quit(&self) -> Result<ordin_service_api::commands::AppQuitResult, ServiceError> {
        let shell = self.handle(&self.modules.shell)?;
        let stopping_sessions = self
            .modules
            .supervisor
            .as_ref()
            .map_or(0, |s| s.running_local_sessions());
        shell.quit(QuitTrigger::AppQuit);
        Ok(ordin_service_api::commands::AppQuitResult { stopping_sessions })
    }

    /// `app.pick_folder`: cancelled → `folder` absent; a picker failure → `DSK_PICKER_UNAVAILABLE`.
    async fn app_pick_folder(
        &self,
    ) -> Result<ordin_service_api::commands::AppPickFolderResult, ServiceError> {
        let shell = self.handle(&self.modules.shell)?;
        let folder = shell
            .pick_folder()
            .await
            .map_err(|_| self.own_refusal(PICKER_UNAVAILABLE, true))?;
        Ok(ordin_service_api::commands::AppPickFolderResult {
            folder: folder.map(|f| f.to_string_lossy().into_owned()),
        })
    }

    /// `settings.get`: `notifications`, `display`, `what_ordin_remembers` (DSK-MEM-1) and `about`; no settings for any other section
    /// (DSK-REC-37).
    fn settings_get(
        &self,
        input: ordin_service_api::commands::SettingsSectionRef,
    ) -> Result<ordin_service_api::commands::SettingsView, ServiceError> {
        let preferences = self.handle(&self.modules.preferences)?;
        preferences
            .section(input.section)
            .ok_or_else(|| self.not_answered())
    }

    /// `settings.change` of a `notifications.*` key or `display.appearance`; a changed appearance
    /// is sent to the window as `ordin://appearance`.
    fn settings_change(
        &self,
        input: ordin_service_api::commands::ChangeSetting,
    ) -> Result<ordin_service_api::commands::SettingView, ServiceError> {
        let preferences = self.handle(&self.modules.preferences)?;
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-37
        if !Preferences::owns(&input.key) {
            return Err(self.refuse(INPUT_INVALID, None));
        }
        let changed = preferences.change(&input.key, &input.value).map_err(|e| {
            self.own_refusal(e.code(), e == crate::prefs::PreferenceError::WriteFailed)
        })?;
        if let (Some(appearance), Some(shell)) = (changed.appearance, &self.modules.shell) {
            shell.appearance_changed(appearance);
        }
        // DSK-MEM-1: the Memory Service enforces Settings › Memory from the next write and recall.
        // The value was checked against the same bounds, so a refusal here is not expected; it is
        // logged and the saved value is given again at the next start.
        // spec: ordin-desktop/components/OrdinDesktop#DSK-MEM-1
        if let (Some(m), Some(memory)) = (changed.memory, &self.modules.memory) {
            if let Err(e) = memory.set_settings(m.settings()) {
                tracing::warn!(
                    component = "desktop",
                    code = e.code(),
                    "memory_settings_not_applied"
                );
            }
        }
        Ok(changed.view)
    }

    /// `app.agent_settings` (DSK-AGM-1): the person's own choices, each level's default and whether
    /// each harness is set up on this computer.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
    async fn app_agent_settings(
        &self,
    ) -> Result<ordin_service_api::commands::AgentSettingsView, ServiceError> {
        let preferences = self.handle(&self.modules.preferences)?;
        let snapshot = self.handle(&self.modules.profile)?.current();
        let agents = preferences.read().agents;
        // An unreadable list of harnesses shows each one as not found; it never blocks the section.
        let attachments = match &self.modules.setup {
            Some(setup) => setup.harness_attachments().await.unwrap_or_default(),
            None => Vec::new(),
        };
        let in_use = self
            .modules
            .harness_in_use
            .unwrap_or_else(|| crate::agent_settings::harness_to_use(&agents, &snapshot));
        Ok(crate::agent_settings::view(
            &agents,
            &snapshot,
            in_use,
            &attachments,
        ))
    }

    /// `app.change_agent_setting` (DSK-AGM-1): the change stored, then the models and efforts given
    /// to the Usage Planner, which uses them from the next step. A harness change takes effect when
    /// the program next starts.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-AGM-1
    async fn app_change_agent_setting(
        &self,
        input: ordin_service_api::commands::ChangeAgentSetting,
    ) -> Result<ordin_service_api::commands::AgentSettingsView, ServiceError> {
        let preferences = self.handle(&self.modules.preferences)?;
        let stored = preferences.change_agents(&input).map_err(|e| {
            self.own_refusal(e.code(), e == crate::prefs::PreferenceError::WriteFailed)
        })?;
        if let Some(usage) = &self.modules.usage {
            usage.set_personal_choices(stored.choices());
        }
        self.app_agent_settings().await
    }

    /// `setup.choose_harness` (DSK-CHO-2, DSK-CHO-4): the answer kept through the Setup Assistant,
    /// then the harness to use worked out again from the stored preferences. The same harness as
    /// the one wired at row 3.19 → `{ restarting: false }`, and the guided run goes on. Another →
    /// `{ restarting: true }`, and the shutdown with `HarnessChosen` starts on its own task, so this
    /// answer is not held back by it.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-4
    async fn setup_choose_harness(
        &self,
        input: ordin_service_api::commands::setup::ChooseHarness,
    ) -> Result<ordin_service_api::commands::setup::ChooseHarnessResult, ServiceError> {
        let setup = self.handle(&self.modules.setup)?;
        setup
            .choose_harness(input.harness)
            .await
            .map_err(|e| self.module_error(e))?;
        let preferences = self.handle(&self.modules.preferences)?;
        let snapshot = self.handle(&self.modules.profile)?.current();
        let to_use = crate::agent_settings::harness_to_use(&preferences.read().agents, &snapshot);
        let in_use = self.modules.harness_in_use.unwrap_or(to_use);
        let restarting = to_use != in_use;
        if restarting {
            tracing::info!(
                component = "desktop",
                harness = ?to_use,
                "harness_chosen_restart"
            );
            if let Some(shell) = &self.modules.shell {
                shell.quit(QuitTrigger::HarnessChosen);
            }
        }
        Ok(ordin_service_api::commands::setup::ChooseHarnessResult { restarting })
    }

    /// `files.save_export`: the name checked, the save dialog, then an atomic UTF-8 write. The text
    /// is never logged.
    async fn files_save_export(
        &self,
        input: ordin_service_api::commands::FilesSaveExportInput,
    ) -> Result<ordin_service_api::commands::SavedCopy, ServiceError> {
        if !crate::shell::export_name_valid(&input.file_name) {
            return Err(self.refuse(INPUT_INVALID, None));
        }
        let shell = self.handle(&self.modules.shell)?;
        let path = shell
            .save_path(&input.file_name)
            .await
            .ok_or_else(|| self.own_refusal(DIALOG_CANCELLED, false))?;
        crate::shell::write_atomically(&path, input.text.as_bytes())
            .map_err(|_| self.own_refusal(SAVE_COPY_FAILED, true))?;
        Ok(ordin_service_api::commands::SavedCopy {
            saved_path: path.to_string_lossy().into_owned(),
        })
    }

    /// `app.open_slack_link`: a Slack permalink opened in the person's browser; anything else →
    /// `DSK_LINK_REFUSED`, nothing opened.
    fn app_open_slack_link(
        &self,
        input: ordin_service_api::commands::AppOpenSlackLinkInput,
    ) -> Result<ordin_service_api::Done, ServiceError> {
        if !crate::shell::is_slack_permalink(&input.url) {
            tracing::warn!(component = "desktop", code = LINK_REFUSED, "link_refused");
            return Err(self.own_refusal(LINK_REFUSED, false));
        }
        let opener = self.handle(&self.modules.url_opener)?;
        // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-38
        opener.open(&input.url).map_err(|_| {
            tracing::warn!(
                component = "desktop",
                command = "app.open_slack_link",
                "link_open_failed"
            );
            self.own_refusal(BROWSER_FAILED, true)
        })?;
        Ok(ordin_service_api::Done {})
    }

    /// `app.open_web_page` (DSK-WEB-1): a page ordin names, opened at its fixed address in the
    /// person's browser; the address is logged nowhere.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-WEB-1
    pub(crate) fn app_open_web_page(
        &self,
        input: ordin_service_api::commands::OpenWebPage,
    ) -> Result<ordin_service_api::Done, ServiceError> {
        let opener = self.handle(&self.modules.url_opener)?;
        opener
            .open(crate::shell::web_page_address(input.page))
            .map_err(|_| {
                tracing::warn!(
                    component = "desktop",
                    command = "app.open_web_page",
                    page = ordin_types::log::wire_word(&input.page).as_str(),
                    "link_open_failed"
                );
                self.own_refusal(BROWSER_FAILED, true)
            })?;
        Ok(ordin_service_api::Done {})
    }

    /// `brand.get`: the active pack, its strings for the active locale and the newest refusal
    /// (Brand.md, *Service API commands*; the pack's fields as the owner keeps them).
    fn brand_get(
        &self,
    ) -> Result<ordin_service_api::commands::brand::BrandGetResult, ServiceError> {
        use ordin_service_api::commands::brand::{
            BrandGetResult, BrandGetResultLinks, BrandGetResultRefusal,
        };
        let brand = self.handle(&self.modules.brand)?;
        let pack = brand.active();
        let refusal = brand.last_refusal().map(|refused| BrandGetResultRefusal {
            brand_identifier: refused.brand_identifier.clone(),
            version: refused.version.clone(),
            reason: refused.refusal_reason.clone().unwrap_or_default(),
            received_at: refused.received_at,
        });
        Ok(BrandGetResult {
            brand_identifier: pack.brand_identifier.clone(),
            version: pack.version.clone(),
            source: pack.source,
            strings: brand.strings(),
            design_tokens: pack.design_tokens.clone(),
            links: BrandGetResultLinks {
                help_url: pack.help_url.clone(),
                support_url: pack.support_url.clone(),
                privacy_url: pack.privacy_url.clone(),
            },
            font_count: u8::try_from(pack.font_file_paths.len()).unwrap_or(u8::MAX),
            refusal,
        })
    }

    /// `service.hello` (DSK-UP-8).
    fn hello(&self) -> Result<ServiceHello, ServiceError> {
        let journal = self.handle(&self.modules.journal)?;
        Ok(ServiceHello {
            api_version: ServiceApiVersion::CURRENT,
            backend: self.handle(&self.modules.backend)?,
            // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-5
            backend_name: self.backend_name(),
            profile: BackendProfile::Desktop,
            last_seq: journal.last_seq(),
            groups: self.groups().iter().map(|g| (*g).to_owned()).collect(),
        })
    }
}

/// Who pressed: the screens of this computer, or a paired desktop.
fn press_origin(ctx: &CallContext) -> PressOrigin {
    match &ctx.origin {
        CallOrigin::LocalScreens => PressOrigin::LocalScreen,
        CallOrigin::PairedDesktop { computer_name, .. } => PressOrigin::PairedDesktop {
            computer_name: computer_name.clone(),
        },
    }
}

include!("service_api_rows.rs");

#[path = "service_api_screens.rs"]
mod screens;

#[path = "service_api_owners.rs"]
pub(crate) mod owners;

#[path = "service_api/team.rs"]
mod team;

#[path = "service_api/dashboards.rs"]
mod dashboards;

#[path = "service_api/knowledge_export.rs"]
mod knowledge_export;

#[path = "service_api/ssh_keys.rs"]
mod ssh_keys;

#[path = "service_api/minutes.rs"]
mod minutes;

#[cfg(all(test, feature = "minutes"))]
#[path = "service_api/minutes_tests.rs"]
mod minutes_tests;

/// The body of one generated method: the rows written here, then the delegations of
/// [`delegate!`], then every other row.
macro_rules! answer {
    ($api:expr, $ctx:expr, $input:expr, service_hello) => {{
        let _ = ($ctx, $input);
        $api.hello()
    }};
    ($api:expr, $ctx:expr, $input:expr, approval_decide) => {{
        let approval = $api.handle(&$api.modules.approval)?;
        let press = $api.restate($input)?;
        let outcome = approval
            .decide(press, press_origin($ctx))
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(outcome)
    }};
    ($api:expr, $ctx:expr, $input:expr, approval_list_pending) => {{
        let _ = $ctx;
        let approval = $api.handle(&$api.modules.approval)?;
        let input = $input;
        let page = approval
            .list_pending(input.workspace_id, input.cursor)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(page)
    }};
    ($api:expr, $ctx:expr, $input:expr, approval_get) => {{
        let _ = $ctx;
        let approval = $api.handle(&$api.modules.approval)?;
        let record = approval
            .get($input.approval_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(record)
    }};
    // `home.view` is HomeAndSearch.md's `home(filter)`.
    ($api:expr, $ctx:expr, $input:expr, home_view) => {{
        let _ = $ctx;
        let home = $api.handle(&$api.modules.home)?;
        let input = $input;
        let view = home
            .home(input.workspace_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(view)
    }};
    // `search.query` is HomeAndSearch.md's `search(query, current)`.
    ($api:expr, $ctx:expr, $input:expr, search_query) => {{
        let _ = $ctx;
        let home = $api.handle(&$api.modules.home)?;
        let input = $input;
        let answer = home
            .search(&input.query, input.current_workspace_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(answer)
    }};
    // `workspaces.links` is WorkspaceService.md's `source_links(id)`.
    ($api:expr, $ctx:expr, $input:expr, workspaces_links) => {{
        let _ = $ctx;
        let workspaces = $api.handle(&$api.modules.workspaces)?;
        let links = workspaces
            .source_links($input.workspace_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(links)
    }};
    // `runs.stop` calls `cancel` (SessionSupervisor.md, *Service API commands*).
    ($api:expr, $ctx:expr, $input:expr, runs_stop) => {{
        let _ = $ctx;
        let supervisor = $api.handle(&$api.modules.supervisor)?;
        supervisor
            .cancel($input.run_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    // `runs.answer` calls `resume_with_decision(Answer)` (SessionSupervisor.md).
    ($api:expr, $ctx:expr, $input:expr, runs_answer) => {{
        let _ = $ctx;
        let supervisor = $api.handle(&$api.modules.supervisor)?;
        let input = $input;
        supervisor
            .resume_with_decision(
                input.run_id,
                ordin_runtime_adapter::ResumeDecision::Answer { text: input.text },
            )
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    // `crews.stop`: the Crew Orchestrator's `kill`.
    ($api:expr, $ctx:expr, $input:expr, crews_stop) => {{
        let _ = $ctx;
        let orchestrator = $api.handle(&$api.modules.orchestrator)?;
        orchestrator
            .kill($input.crew_run_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    // `crews.continue`: `continue_crew` with the coordinator from the Ordin Host's `coordinator()`.
    ($api:expr, $ctx:expr, $input:expr, crews_continue) => {{
        let _ = $ctx;
        let orchestrator = $api.handle(&$api.modules.orchestrator)?;
        let host = $api.handle(&$api.modules.ordin_host)?;
        orchestrator
            .continue_crew($input.crew_run_id, host.coordinator())
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::Done {})
    }};
    // `crews.start` (Ordin Host): the person pressed **Start** on the crew plan.
    ($api:expr, $ctx:expr, $input:expr, crews_start) => {{
        let _ = $ctx;
        let host = $api.handle(&$api.modules.ordin_host)?;
        let crew_run_id = host
            .crews_start($input.crew_charter_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::commands::crews::CrewsStartResult { crew_run_id })
    }};
    // `crews.charter` (Ordin Host, OH-WR-2): the charter of a crew plan by its id.
    // spec: ordin-desktop/components/OrdinHost#OH-WR-2
    ($api:expr, $ctx:expr, $input:expr, crews_charter) => {{
        let _ = $ctx;
        let host = $api.handle(&$api.modules.ordin_host)?;
        let charter = host
            .crews_charter($input.crew_charter_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        Ok(::ordin_service_api::commands::crews::CrewsCharterResult { charter })
    }};
    // `profile.get_document`: `current()` as `{ source: "verified" | "defaults", profile_version?,
    // document }` (OrganisationProfile.md, *Service API commands*).
    ($api:expr, $ctx:expr, $input:expr, profile_get_document) => {{
        let _ = ($ctx, $input);
        let profile = $api.handle(&$api.modules.profile)?;
        let snapshot = profile.current();
        let (source, profile_version) = match &snapshot.source {
            ordin_organisation_profile::SnapshotSource::Verified {
                profile_version, ..
            } => ("verified", Some(profile_version.clone())),
            ordin_organisation_profile::SnapshotSource::Defaults => ("defaults", None),
        };
        let mut obj = ::serde_json::Map::new();
        obj.insert(
            "source".to_owned(),
            ::serde_json::Value::String(source.to_owned()),
        );
        if let Some(version) = profile_version {
            obj.insert(
                "profile_version".to_owned(),
                ::serde_json::Value::String(version),
            );
        }
        obj.insert("document".to_owned(), $api.value(&snapshot.document)?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    // The *place* rows of the Memory Service (MemoryService.md, *Service API commands*).
    ($api:expr, $ctx:expr, $input:expr, memory_things) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .list_things(place, input.search, input.cursor)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_thing) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .get_thing(place, input.id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_used) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let recall_ids = $api.restate(input.recall_ids)?;
        let out = memory
            .memory_used(place, recall_ids)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_thing_count) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .count_thing(place, input.id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_forget_thing) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let expected = $api.restate(input.expected)?;
        let out = memory
            .forget_thing(place, input.id, expected)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_same_as) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .same_as(place, input.keep, input.merge)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    // The words, conflicts, passages and the diagram (MemoryService.md amendment ADR-118, ADR-120;
    // ServiceApi.md SVC-TR-1).
    ($api:expr, $ctx:expr, $input:expr, memory_vocabulary) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .vocabulary(place)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_rename_term) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .rename_term(place, input.key, input.label)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_merge_term) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .merge_term(place, input.key, input.into)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    // Topics of things (MemoryService.md MEM-TOP-4, MEM-TOP-5; ServiceApi.md SVC-TOP-1).
    ($api:expr, $ctx:expr, $input:expr, memory_thing_topics) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .thing_topics(place)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_set_thing_topic) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let place = $api.memory_place(input.workspace_id, input.scope_id)?;
        let out = memory
            .set_thing_topic(place, input.id, input.topic)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_keep_conflict_side) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let out = memory
            .keep_conflict_side(input.keep, input.other)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_passages) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let out = memory
            .passages(input.workspace_id, input.page_id)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    ($api:expr, $ctx:expr, $input:expr, memory_diagram) => {{
        let _ = $ctx;
        let memory = $api.handle(&$api.modules.memory)?;
        let input = $input;
        let out = memory
            .diagram(input.workspace_id, input.centre)
            .await
            .map_err(|e| $api.module_error(e))?;
        $api.restate(out)
    }};
    // `localisation.list_languages`: `languages()` and `areas()` (Localisation.md, ADR-109).
    ($api:expr, $ctx:expr, $input:expr, localisation_list_languages) => {{
        let _ = ($ctx, $input);
        let localisation = $api.handle(&$api.modules.localisation)?;
        let mut obj = ::serde_json::Map::new();
        obj.insert(
            "languages".to_owned(),
            $api.value(localisation.languages())?,
        );
        obj.insert("areas".to_owned(), $api.value(localisation.areas())?);
        $api.restate(::serde_json::Value::Object(obj))
    }};
    // `localisation.get_settings`: one `active()` read with `offered()`; the language follows the
    // operating system exactly when the active locale's source is the operating system.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-32
    // spec: ordin-desktop/components/Localisation#LOC-REC-1
    ($api:expr, $ctx:expr, $input:expr, localisation_get_settings) => {{
        let _ = ($ctx, $input);
        let localisation = $api.handle(&$api.modules.localisation)?;
        let active = localisation.active();
        let following_operating_system =
            active.source == ordin_types::domain::LocaleSource::OperatingSystem;
        let mut obj = ::serde_json::Map::new();
        obj.insert("active".to_owned(), $api.value(&active)?);
        obj.insert("offered".to_owned(), $api.value(localisation.offered())?);
        obj.insert(
            "following_operating_system".to_owned(),
            ::serde_json::Value::Bool(following_operating_system),
        );
        $api.restate(::serde_json::Value::Object(obj))
    }};
    ($api:expr, $ctx:expr, $input:expr, brand_get) => {{
        let _ = ($ctx, $input);
        $api.brand_get()
    }};
    ($api:expr, $ctx:expr, $input:expr, app_quit) => {{
        let _ = ($ctx, $input);
        $api.app_quit()
    }};
    ($api:expr, $ctx:expr, $input:expr, app_pick_folder) => {{
        let _ = ($ctx, $input);
        $api.app_pick_folder().await
    }};
    ($api:expr, $ctx:expr, $input:expr, settings_get) => {{
        let _ = $ctx;
        $api.settings_get($input)
    }};
    ($api:expr, $ctx:expr, $input:expr, settings_change) => {{
        let _ = $ctx;
        $api.settings_change($input)
    }};
    ($api:expr, $ctx:expr, $input:expr, files_save_export) => {{
        let _ = $ctx;
        $api.files_save_export($input).await
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-1
    ($api:expr, $ctx:expr, $input:expr, dashboards_export) => {{
        let _ = $ctx;
        $api.dashboards_export($input).await
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-DBX-2
    ($api:expr, $ctx:expr, $input:expr, dashboards_add_to_presentation) => {{
        let _ = $ctx;
        $api.dashboards_add_to_presentation($input).await
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-KEX-1
    ($api:expr, $ctx:expr, $input:expr, knowledge_export) => {{
        let _ = $ctx;
        $api.knowledge_export($input).await
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
    ($api:expr, $ctx:expr, $input:expr, app_ssh_keys) => {{
        let _ = ($ctx, $input);
        $api.app_ssh_keys()
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-2
    ($api:expr, $ctx:expr, $input:expr, app_create_ssh_key) => {{
        let _ = ($ctx, $input);
        $api.app_create_ssh_key().await
    }};
    ($api:expr, $ctx:expr, $input:expr, app_choose_ssh_key) => {{
        let _ = $ctx;
        $api.app_choose_ssh_key($input)
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-3
    ($api:expr, $ctx:expr, $input:expr, app_check_ssh_key) => {{
        let _ = ($ctx, $input);
        $api.app_check_ssh_key().await
    }};
    ($api:expr, $ctx:expr, $input:expr, app_open_slack_link) => {{
        let _ = $ctx;
        $api.app_open_slack_link($input)
    }};
    ($api:expr, $ctx:expr, $input:expr, app_open_web_page) => {{
        let _ = $ctx;
        $api.app_open_web_page($input)
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MD-12
    ($api:expr, $ctx:expr, $input:expr, app_memory_models) => {{
        let _ = ($ctx, $input);
        Ok($api.handle(&$api.modules.memory_models)?.view())
    }};
    ($api:expr, $ctx:expr, $input:expr, app_download_memory_models) => {{
        let _ = ($ctx, $input);
        $api.handle(&$api.modules.memory_models)?
            .download()
            .map_err(|code| $api.own_refusal(code, false))
    }};
    ($api:expr, $ctx:expr, $input:expr, app_stop_memory_models_download) => {{
        let _ = ($ctx, $input);
        Ok($api.handle(&$api.modules.memory_models)?.stop())
    }};
    ($api:expr, $ctx:expr, $input:expr, app_agent_settings) => {{
        let _ = ($ctx, $input);
        $api.app_agent_settings().await
    }};
    ($api:expr, $ctx:expr, $input:expr, app_change_agent_setting) => {{
        let _ = $ctx;
        $api.app_change_agent_setting($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, app_decline_memory_models) => {{
        let _ = ($ctx, $input);
        Ok($api.handle(&$api.modules.memory_models)?.decline())
    }};
    // `setup.install_update`, then step 15 when it succeeded (DSK-UP-10).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-10
    ($api:expr, $ctx:expr, $input:expr, setup_install_update) => {{
        let _ = ($ctx, $input);
        let setup = $api.handle(&$api.modules.setup)?;
        setup
            .install_update()
            .await
            .map_err(|e| $api.module_error(e))?;
        if let Some(shell) = &$api.modules.shell {
            shell.quit(QuitTrigger::Update);
        }
        Ok(::ordin_service_api::Done {})
    }};
    // spec: ordin-desktop/components/OrdinDesktop#DSK-CHO-2
    ($api:expr, $ctx:expr, $input:expr, setup_choose_harness) => {{
        let _ = $ctx;
        $api.setup_choose_harness($input).await
    }};
    // The rows whose owner takes an argument the program fills (SVC-R1-1).
    ($api:expr, $ctx:expr, $input:expr, agents_installed_files) => {{
        let _ = $ctx;
        $api.agents_installed_files($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_installed_file) => {{
        let _ = $ctx;
        $api.agents_installed_file($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, agents_part_page) => {{
        let _ = $ctx;
        $api.agents_part_page($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, checker_check_installed) => {{
        let _ = $ctx;
        $api.checker_check_installed($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, checker_check_library_item) => {{
        let _ = $ctx;
        $api.checker_check_library_item($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_sign_in) => {{
        let _ = $ctx;
        $api.identity_sign_in($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_sign_in_wait) => {{
        let _ = $ctx;
        $api.identity_sign_in_wait($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_save_mail_password) => {{
        let _ = $ctx;
        $api.identity_save_mail_password($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_save_gitlab_token) => {{
        let _ = $ctx;
        $api.identity_save_gitlab_token($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_save_github_token) => {{
        let _ = $ctx;
        $api.identity_save_github_token($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, confluence_publish_checked) => {{
        let _ = $ctx;
        $api.confluence_publish_checked($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, files_save_edit) => {{
        let _ = $ctx;
        $api.files_save_edit($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, identity_import_client_certificate) => {{
        let _ = ($ctx, $input);
        $api.identity_import_client_certificate().await
    }};
    ($api:expr, $ctx:expr, $input:expr, files_read_version) => {{
        let _ = $ctx;
        $api.files_read_version($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, files_download) => {{
        let _ = $ctx;
        let input = $input;
        $api.files_download(input.place_id, &input.path).await
    }};
    ($api:expr, $ctx:expr, $input:expr, files_save_copy) => {{
        let _ = $ctx;
        $api.files_save_copy($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, diagnostics_support_export) => {{
        let _ = $ctx;
        $api.diagnostics_support_export($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, diagnostics_clear_logs) => {{
        let _ = ($ctx, $input);
        $api.diagnostics_clear_logs().await
    }};
    ($api:expr, $ctx:expr, $input:expr, workspaces_tree) => {{
        let _ = $ctx;
        $api.workspaces_tree($input.workspace_id).await
    }};
    // The `team_knowledge.*` commands (DSK-TW-3), in `service_api/team.rs`.
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_status) => {{
        let _ = $ctx;
        $api.tk_status($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_candidate_links) => {{
        let _ = $ctx;
        $api.tk_candidate_links($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_turn_on) => {{
        let _ = $ctx;
        $api.tk_turn_on($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_turn_off) => {{
        let _ = $ctx;
        $api.tk_turn_off($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_check_now) => {{
        let _ = $ctx;
        $api.tk_check_now($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_get) => {{
        let _ = $ctx;
        $api.tk_get($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_share) => {{
        let _ = $ctx;
        $api.tk_share($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_workspace_share_preview) => {{
        let _ = $ctx;
        $api.tk_workspace_share_preview($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_share_workspace) => {{
        let _ = $ctx;
        $api.tk_share_workspace($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_workspace_open_preview) => {{
        let _ = $ctx;
        $api.tk_workspace_open_preview($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_open_workspace) => {{
        let _ = $ctx;
        $api.tk_open_workspace($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_pending_grants) => {{
        let _ = $ctx;
        $api.tk_pending_grants($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_allow_grant) => {{
        let _ = $ctx;
        $api.tk_allow_grant($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_decline_grant) => {{
        let _ = $ctx;
        $api.tk_decline_grant($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_activity) => {{
        let _ = $ctx;
        $api.tk_activity($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_results_choices) => {{
        let _ = $ctx;
        $api.tk_results_choices($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_set_results_place) => {{
        let _ = $ctx;
        $api.tk_set_results_place($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, team_knowledge_leave_workspace) => {{
        let _ = $ctx;
        $api.tk_leave_workspace($input).await
    }};
    // The `minutes.*` commands (DSK-MIN-1 to DSK-MIN-5), in `service_api/minutes.rs`.
    ($api:expr, $ctx:expr, $input:expr, minutes_status) => {{
        let _ = $ctx;
        $api.min_status($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_list) => {{
        let _ = $ctx;
        $api.min_list($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_get) => {{
        let _ = $ctx;
        $api.min_get($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_transcript) => {{
        let _ = $ctx;
        $api.min_transcript($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_save_edit) => {{
        let _ = $ctx;
        $api.min_save_edit($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_remove) => {{
        let _ = $ctx;
        $api.min_remove($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_import) => {{
        let _ = $ctx;
        $api.min_import($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_jobs) => {{
        let _ = $ctx;
        $api.min_jobs($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_cancel_job) => {{
        let _ = $ctx;
        $api.min_cancel_job($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_retry) => {{
        let _ = $ctx;
        $api.min_retry($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_give_model_consent) => {{
        let _ = ($ctx, $input);
        $api.min_give_model_consent().await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_remove_model) => {{
        let _ = ($ctx, $input);
        $api.min_remove_model().await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_accept_notice) => {{
        let _ = ($ctx, $input);
        $api.min_accept_notice().await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_change_settings) => {{
        let _ = $ctx;
        $api.min_change_settings($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_record) => {{
        let _ = $ctx;
        $api.min_record($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_stop) => {{
        let _ = $ctx;
        $api.min_stop($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_open_obs) => {{
        let _ = ($ctx, $input);
        $api.min_open_obs().await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_save_obs_password) => {{
        let _ = $ctx;
        $api.min_save_obs_password($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_delete_recording) => {{
        let _ = $ctx;
        $api.min_delete_recording($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, minutes_export) => {{
        let _ = $ctx;
        $api.min_export($input).await
    }};
    ($api:expr, $ctx:expr, $input:expr, $other:ident) => {{
        delegate!($api, $ctx, $input, $other)
    }};
}

macro_rules! desktop_methods {
    ($( ($module:ident, $verb:ident, $method:ident, $input:ty, $result:ty, $scope:ident, $effect:ident); )*) => {
        #[async_trait::async_trait]
        impl ServiceApi for DesktopServiceApi {
            $(
                async fn $method(
                    &self,
                    ctx: &CallContext,
                    input: $input,
                ) -> Result<$result, ServiceError> {
                    answer!(self, ctx, input, $method)
                }
            )*

            // spec: ordin-desktop/components/OrdinDesktop#DSK-UP-7
            fn groups(&self) -> &'static [&'static str] {
                crate::groups::groups()
            }
        }
    };
}

ordin_service_api::for_each_command!(desktop_methods);
