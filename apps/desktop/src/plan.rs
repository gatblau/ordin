//! The order of *Internal Logic* step 3 as data (OrdinDesktop.md step 3 and the amendment January
//! 2026, DSK-UP-5 and DSK-UP-6): which rows this build constructs, in which order, and the wiring
//! steps that sit between them. The numbering is the spec's; a row behind a switch the build lacks
//! is skipped (DSK-UP-4).

/// One row of the table of step 3. The string is the row number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Row {
    Journal,
    OrganisationProfile,
    Identity,
    Approval,
    Localisation,
    AgentFolders,
    FormatPlugins,
    Resolver,
    SharePoint,
    #[cfg(feature = "hosted-agents")]
    HostedAgentAdapter,
    #[cfg(feature = "slack")]
    SlackThreadAdapter,
    UsagePlanner,
    ServerConnector,
    #[cfg(feature = "knowledge-agents")]
    KnowledgeQueryClient,
    Rules,
    Brand,
    #[cfg(feature = "dictation")]
    Dictation,
    #[cfg(feature = "minutes")]
    Transcription,
    #[cfg(feature = "minutes")]
    ObsRecorder,
    Checker,
    Installers,
    SessionSupervisor,
    McpFront,
    PackageInspector,
    Files,
    HookRelay,
    CrewOrchestrator,
    #[cfg(feature = "sandbox")]
    Sandbox,
    MemoryService,
    CraftToolkit,
    Confluence,
    Jira,
    Projects,
    ClaudeAdapter,
    #[cfg(feature = "copilot")]
    CopilotAdapter,
    Workspaces,
    Catalogue,
    Push,
    Knowledge,
    #[cfg(feature = "mail")]
    Mail,
    #[cfg(feature = "slack")]
    SlackService,
    PackageCatalogue,
    OrdinHost,
    #[cfg(feature = "team-knowledge")]
    TeamKnowledge,
    #[cfg(feature = "minutes")]
    Minutes,
    HomeSearch,
    Setup,
    #[cfg(feature = "authoring")]
    Authoring,
    #[cfg(feature = "handover")]
    Handover,
    #[cfg(feature = "telegram")]
    TelegramAdapter,
    #[cfg(feature = "telegram")]
    TelegramService,
    ServiceApi,
    CommandBridge,
}

impl Row {
    /// The row number of the table of step 3.
    pub fn number(self) -> &'static str {
        match self {
            Row::Journal => "3.1",
            Row::OrganisationProfile => "3.2",
            Row::Identity => "3.3",
            Row::Approval => "3.4",
            Row::Localisation => "3.5",
            Row::AgentFolders => "3.6",
            Row::FormatPlugins => "3.7",
            Row::Resolver => "3.8",
            Row::SharePoint => "3.9",
            #[cfg(feature = "hosted-agents")]
            Row::HostedAgentAdapter => "3.10",
            #[cfg(feature = "slack")]
            Row::SlackThreadAdapter => "3.11",
            Row::UsagePlanner => "3.12",
            Row::ServerConnector => "3.13",
            #[cfg(feature = "knowledge-agents")]
            Row::KnowledgeQueryClient => "3.13a",
            Row::Rules => "3.14",
            Row::Brand => "3.15",
            #[cfg(feature = "dictation")]
            Row::Dictation => "3.16",
            #[cfg(feature = "minutes")]
            Row::Transcription => "3.16a",
            #[cfg(feature = "minutes")]
            Row::ObsRecorder => "3.16b",
            Row::Checker => "3.17",
            Row::Installers => "3.18",
            Row::SessionSupervisor => "3.19",
            Row::McpFront => "3.20",
            Row::PackageInspector => "3.21",
            Row::Files => "3.22",
            Row::HookRelay => "3.23",
            Row::CrewOrchestrator => "3.24",
            #[cfg(feature = "sandbox")]
            Row::Sandbox => "3.27",
            Row::MemoryService => "3.28",
            Row::CraftToolkit => "3.29",
            Row::Confluence => "3.30",
            Row::Jira => "3.30a",
            Row::Projects => "3.31",
            Row::ClaudeAdapter => "3.32",
            #[cfg(feature = "copilot")]
            Row::CopilotAdapter => "3.33",
            Row::Workspaces => "3.35",
            Row::Catalogue => "3.36",
            Row::Push => "3.37",
            Row::Knowledge => "3.38",
            #[cfg(feature = "mail")]
            Row::Mail => "3.39",
            #[cfg(feature = "slack")]
            Row::SlackService => "3.39a",
            Row::PackageCatalogue => "3.40",
            Row::OrdinHost => "3.41",
            #[cfg(feature = "team-knowledge")]
            Row::TeamKnowledge => "3.41a",
            // The spec numbers the Minutes Service 3.41a too (DSK-MIN-1); it is built after the
            // Team Knowledge Service.
            #[cfg(feature = "minutes")]
            Row::Minutes => "3.41a",
            Row::HomeSearch => "3.42",
            Row::Setup => "3.43",
            #[cfg(feature = "authoring")]
            Row::Authoring => "3.44",
            #[cfg(feature = "handover")]
            Row::Handover => "3.45",
            #[cfg(feature = "telegram")]
            Row::TelegramAdapter => "3.46",
            #[cfg(feature = "telegram")]
            Row::TelegramService => "3.47",
            Row::ServiceApi => "3.48",
            Row::CommandBridge => "3.49",
        }
    }

    /// The crate the row constructs; the `module` of `StartError::ModuleFailed`.
    pub fn module(self) -> &'static str {
        match self {
            Row::Journal => "ordin-journal",
            Row::OrganisationProfile => "ordin-organisation-profile",
            Row::Identity => "ordin-identity",
            Row::Approval => "ordin-approval",
            Row::Localisation => "ordin-localisation",
            Row::AgentFolders => "ordin-agent-folders",
            Row::FormatPlugins => "ordin-format-plugins",
            Row::Resolver => "ordin-resolver",
            Row::SharePoint => "ordin-sharepoint",
            #[cfg(feature = "hosted-agents")]
            Row::HostedAgentAdapter => "ordin-hosted-agent-adapter",
            #[cfg(feature = "slack")]
            Row::SlackThreadAdapter => "ordin-slack-thread-adapter",
            Row::UsagePlanner => "ordin-usage-planner",
            Row::ServerConnector => "ordin-server-connector",
            #[cfg(feature = "knowledge-agents")]
            Row::KnowledgeQueryClient => "ordin-knowledge-query-client",
            Row::Rules => "ordin-rules",
            Row::Brand => "ordin-brand",
            #[cfg(feature = "dictation")]
            Row::Dictation => "ordin-dictation",
            #[cfg(feature = "minutes")]
            Row::Transcription => "ordin-dictation",
            #[cfg(feature = "minutes")]
            Row::ObsRecorder => "ordin-obs",
            Row::Checker => "ordin-checker",
            Row::Installers => "ordin-installers",
            Row::SessionSupervisor => "ordin-session-supervisor",
            Row::McpFront => "ordin-mcp-front",
            Row::PackageInspector => "ordin-package-inspector",
            Row::Files => "ordin-files",
            Row::HookRelay => "ordin-hook-relay",
            Row::CrewOrchestrator => "ordin-crew-orchestrator",
            #[cfg(feature = "sandbox")]
            Row::Sandbox => "ordin-sandbox",
            Row::MemoryService => "ordin-memory-service",
            Row::CraftToolkit => "ordin-craft-toolkit",
            Row::Confluence => "ordin-confluence",
            Row::Jira => "ordin-jira",
            Row::Projects => "ordin-projects",
            Row::ClaudeAdapter => "ordin-claude-adapter",
            #[cfg(feature = "copilot")]
            Row::CopilotAdapter => "ordin-copilot-adapter",
            Row::Workspaces => "ordin-workspaces",
            Row::Catalogue => "ordin-catalogue",
            Row::Push => "ordin-push",
            Row::Knowledge => "ordin-knowledge",
            #[cfg(feature = "mail")]
            Row::Mail => "ordin-mail",
            #[cfg(feature = "slack")]
            Row::SlackService => "ordin-slack-service",
            Row::PackageCatalogue => "ordin-package-catalogue",
            Row::OrdinHost => "ordin-host",
            #[cfg(feature = "team-knowledge")]
            Row::TeamKnowledge => "ordin-team-knowledge",
            #[cfg(feature = "minutes")]
            Row::Minutes => "ordin-minutes",
            Row::HomeSearch => "ordin-home-search",
            Row::Setup => "ordin-setup",
            #[cfg(feature = "authoring")]
            Row::Authoring => "ordin-authoring",
            #[cfg(feature = "handover")]
            Row::Handover => "ordin-handover",
            #[cfg(feature = "telegram")]
            Row::TelegramAdapter => "ordin-telegram-adapter",
            #[cfg(feature = "telegram")]
            Row::TelegramService => "ordin-telegram-service",
            Row::ServiceApi => "ordin-desktop",
            Row::CommandBridge => "ordin-command-bridge",
        }
    }
}

/// A provider registered with the MCP Front before `McpFront::start` (DSK-UP-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    #[cfg(feature = "sandbox")]
    Sandbox,
    MemoryService,
    CraftToolkit,
    Projects,
    Knowledge,
    #[cfg(feature = "mail")]
    Mail,
    #[cfg(feature = "slack")]
    SlackService,
    OrdinHost,
    #[cfg(feature = "authoring")]
    Authoring,
    #[cfg(feature = "telegram")]
    TelegramService,
}

/// A runtime adapter registered with the Session Supervisor (DSK-UP-5, rows 3.32–3.34).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    Claude,
    #[cfg(feature = "copilot")]
    Copilot,
    #[cfg(feature = "hosted-agents")]
    HostedAgent,
    #[cfg(feature = "slack")]
    SlackThread,
}

/// One step of `Core::start`'s construction, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Construct the row's module with the owner's constructor.
    Construct(Row),
    /// `mcp_front.register(<provider>.tool_provider())`.
    RegisterProvider(Provider),
    /// `session_supervisor.register_adapter(<adapter>)`.
    RegisterAdapter(Adapter),
    /// `usage.set_harnesses(..)` with the local adapters this build has (DSK-UP-5, first seam).
    SetHarnesses,
    /// `resolver.install_part_source(catalogue)` then `checker.install_part_source(catalogue)` (R18).
    BindPartSource,
    /// `mcp_front.bind_block_store(ordin_host.block_store())` (row 3.41).
    BindBlockStore,
    /// `mcp_front.bind_ref_resolver(<RefResolver>)` (SA-CLOSE-01).
    BindRefResolver,
    /// `mcp_front.start(shutdown)`, which freezes the registry.
    StartMcpFront,
    /// `setup_assistant.register_updater(TauriUpdater)` (row 3.43).
    RegisterUpdater,
    /// `setup_assistant.register_probe(<Copilot HarnessProbe>)` (row 3.43, `copilot` only).
    #[cfg(feature = "copilot")]
    RegisterCopilotProbe,
}

/// Rows 3.27–3.47: the last of them this build constructs is followed by `bind_ref_resolver` and
/// `McpFront::start` (DSK-UP-5, seam *`bind_ref_resolver` and `McpFront::start`*).
fn closes_provider_rows(row: Row) -> bool {
    #[cfg(feature = "telegram")]
    {
        row == Row::TelegramService
    }
    #[cfg(all(not(feature = "telegram"), feature = "handover"))]
    {
        row == Row::Handover
    }
    #[cfg(all(
        not(feature = "telegram"),
        not(feature = "handover"),
        feature = "authoring"
    ))]
    {
        row == Row::Authoring
    }
    #[cfg(all(
        not(feature = "telegram"),
        not(feature = "handover"),
        not(feature = "authoring")
    ))]
    {
        row == Row::Setup
    }
}

/// The construction steps of this build, in order (step 3 with DSK-UP-5 and DSK-UP-6).
pub fn start_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    for row in rows() {
        push_row(&mut steps, row);
    }
    steps
}

/// Row `row` and the wiring done right after it.
fn push_row(steps: &mut Vec<Step>, row: Row) {
    use Step::*;
    steps.push(Construct(row));
    match row {
        #[cfg(feature = "sandbox")]
        Row::Sandbox => steps.push(RegisterProvider(Provider::Sandbox)),
        Row::MemoryService => steps.push(RegisterProvider(Provider::MemoryService)),
        Row::CraftToolkit => steps.push(RegisterProvider(Provider::CraftToolkit)),
        Row::Projects => steps.push(RegisterProvider(Provider::Projects)),
        Row::ClaudeAdapter => {
            steps.push(RegisterAdapter(Adapter::Claude));
            // `copilot` off: called after row 3.32 with the Claude Local Adapter only.
            #[cfg(not(feature = "copilot"))]
            steps.push(SetHarnesses);
        }
        #[cfg(feature = "copilot")]
        Row::CopilotAdapter => {
            steps.push(RegisterAdapter(Adapter::Copilot));
            steps.push(SetHarnesses);
            // Row 3.34.
            #[cfg(feature = "hosted-agents")]
            steps.push(RegisterAdapter(Adapter::HostedAgent));
            #[cfg(feature = "slack")]
            steps.push(RegisterAdapter(Adapter::SlackThread));
        }
        Row::Catalogue => steps.push(BindPartSource),
        Row::Knowledge => steps.push(RegisterProvider(Provider::Knowledge)),
        #[cfg(feature = "mail")]
        Row::Mail => steps.push(RegisterProvider(Provider::Mail)),
        #[cfg(feature = "slack")]
        Row::SlackService => steps.push(RegisterProvider(Provider::SlackService)),
        Row::OrdinHost => {
            steps.push(RegisterProvider(Provider::OrdinHost));
            steps.push(BindBlockStore);
        }
        Row::Setup => {
            steps.push(RegisterUpdater);
            #[cfg(feature = "copilot")]
            steps.push(RegisterCopilotProbe);
        }
        #[cfg(feature = "authoring")]
        Row::Authoring => steps.push(RegisterProvider(Provider::Authoring)),
        #[cfg(feature = "telegram")]
        Row::TelegramService => steps.push(RegisterProvider(Provider::TelegramService)),
        _ => {}
    }
    // Row 3.34 with `copilot` off: the remote adapters follow the Claude Local Adapter.
    #[cfg(not(feature = "copilot"))]
    if row == Row::ClaudeAdapter {
        #[cfg(feature = "hosted-agents")]
        steps.push(RegisterAdapter(Adapter::HostedAgent));
        #[cfg(feature = "slack")]
        steps.push(RegisterAdapter(Adapter::SlackThread));
    }
    if closes_provider_rows(row) {
        steps.push(BindRefResolver);
        steps.push(StartMcpFront);
    }
}

/// The rows of step 3 this build constructs, in order.
pub fn rows() -> Vec<Row> {
    use Row::*;
    let mut rows = vec![
        Journal,
        OrganisationProfile,
        Identity,
        Approval,
        Localisation,
        AgentFolders,
        FormatPlugins,
        Resolver,
        SharePoint,
    ];
    #[cfg(feature = "hosted-agents")]
    rows.push(HostedAgentAdapter);
    #[cfg(feature = "slack")]
    rows.push(SlackThreadAdapter);
    rows.extend([UsagePlanner, ServerConnector]);
    #[cfg(feature = "knowledge-agents")]
    rows.push(KnowledgeQueryClient);
    rows.extend([Rules, Brand]);
    #[cfg(feature = "dictation")]
    rows.push(Dictation);
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-1
    #[cfg(feature = "minutes")]
    rows.extend([Transcription, ObsRecorder]);
    rows.extend([
        Checker,
        Installers,
        SessionSupervisor,
        McpFront,
        PackageInspector,
        Files,
        HookRelay,
        CrewOrchestrator,
    ]);
    #[cfg(feature = "sandbox")]
    rows.push(Sandbox);
    rows.extend([
        MemoryService,
        CraftToolkit,
        Confluence,
        Jira,
        Projects,
        ClaudeAdapter,
    ]);
    #[cfg(feature = "copilot")]
    rows.push(CopilotAdapter);
    rows.extend([Workspaces, Catalogue, Push, Knowledge]);
    #[cfg(feature = "mail")]
    rows.push(Mail);
    #[cfg(feature = "slack")]
    rows.push(SlackService);
    rows.extend([PackageCatalogue, OrdinHost]);
    #[cfg(feature = "team-knowledge")]
    rows.push(TeamKnowledge);
    #[cfg(feature = "minutes")]
    rows.push(Minutes);
    rows.extend([HomeSearch, Setup]);
    #[cfg(feature = "authoring")]
    rows.push(Authoring);
    #[cfg(feature = "handover")]
    rows.push(Handover);
    #[cfg(feature = "telegram")]
    rows.extend([TelegramAdapter, TelegramService]);
    rows.extend([ServiceApi, CommandBridge]);
    rows
}

/// `SetupConfig.bundled_programs` by name: `ordin-hook` in every build (DSK-CPX-5, SET-CPX-1).
// spec: ordin-desktop/components/OrdinDesktop#DSK-CPX-5
pub fn bundled_program_names() -> Vec<&'static str> {
    vec!["ordin-hook"]
}

/// The build switches of this build, for the `start` log line's `features` (amendment January 2026,
/// *Observability*).
pub fn features() -> Vec<&'static str> {
    let mut on: Vec<&'static str> = Vec::new();
    for (name, enabled) in [
        ("authoring", cfg!(feature = "authoring")),
        ("copilot", cfg!(feature = "copilot")),
        ("dictation", cfg!(feature = "dictation")),
        ("handover", cfg!(feature = "handover")),
        ("hosted-agents", cfg!(feature = "hosted-agents")),
        ("knowledge-agents", cfg!(feature = "knowledge-agents")),
        ("mail", cfg!(feature = "mail")),
        ("minutes", cfg!(feature = "minutes")),
        ("sandbox", cfg!(feature = "sandbox")),
        ("slack", cfg!(feature = "slack")),
        ("team-knowledge", cfg!(feature = "team-knowledge")),
        ("telegram", cfg!(feature = "telegram")),
    ] {
        if enabled {
            on.push(name);
        }
    }
    on
}

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: Happy path — a build with minutes wires the three modules (DSK-MIN-6: without dictation)
    // spec: ordin-desktop/components/OrdinDesktop#DSK-MIN-6
    #[cfg(all(feature = "minutes", not(feature = "dictation")))]
    #[test]
    fn minutes_does_not_turn_dictation_on() {
        let on = features();
        assert!(on.contains(&"minutes"));
        assert!(!on.contains(&"dictation"));
        let rows = numbers(&rows());
        assert!(rows.contains(&"3.16a") && rows.contains(&"3.16b"));
        assert!(!rows.contains(&"3.16"));
    }

    fn numbers(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.number()).collect()
    }

    fn position(steps: &[Step], step: Step) -> usize {
        steps
            .iter()
            .position(|s| *s == step)
            .unwrap_or_else(|| panic!("{step:?} is a step"))
    }

    /// Row numbers in the spec's order, so that "in order" is checked against the numbering.
    fn order_key(number: &str) -> (u32, u32) {
        let rest = number.strip_prefix("3.").expect("3.x");
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let suffix = rest[digits.len()..].chars().next().map_or(0, |c| c as u32);
        (digits.parse().expect("number"), suffix)
    }

    #[test]
    fn rows_follow_the_numbering_of_step_3() {
        let rows = rows();
        let keys: Vec<_> = rows.iter().map(|r| order_key(r.number())).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(rows.first(), Some(&Row::Journal));
        assert_eq!(rows.last(), Some(&Row::CommandBridge));
        // Rows 3.25 and 3.26 are unused; 3.34 constructs nothing.
        for unused in ["3.25", "3.26", "3.34"] {
            assert!(!numbers(&rows).contains(&unused), "{unused}");
        }
    }

    // OrdinDesktop.md amendment January 2026 (ADR-114).
    #[cfg(not(any(
        feature = "sandbox",
        feature = "dictation",
        feature = "copilot",
        feature = "slack",
        feature = "mail",
        feature = "team-knowledge",
        feature = "authoring",
        feature = "handover",
        feature = "telegram",
        feature = "hosted-agents",
        feature = "knowledge-agents"
    )))]
    #[test]
    fn happy_path_a_build_with_no_switch_wires_its_modules_and_shows_the_window() {
        let rows = rows();
        let built = numbers(&rows);
        assert_eq!(
            built,
            [
                "3.1", "3.2", "3.3", "3.4", "3.5", "3.6", "3.7", "3.8", "3.9", "3.12", "3.13",
                "3.14", "3.15", "3.17", "3.18", "3.19", "3.20", "3.21", "3.22", "3.23", "3.24",
                "3.28", "3.29", "3.30", "3.30a", "3.31", "3.32", "3.35", "3.36", "3.37", "3.38",
                "3.40", "3.41", "3.42", "3.43", "3.48", "3.49"
            ]
        );
        for absent in [
            "3.10", "3.11", "3.13a", "3.16", "3.27", "3.33", "3.39", "3.39a", "3.41a", "3.44",
            "3.45", "3.46", "3.47",
        ] {
            assert!(!built.contains(&absent), "{absent} is not constructed");
        }
        let steps = start_steps();
        let start = position(&steps, Step::StartMcpFront);
        let providers: Vec<_> = steps[..start]
            .iter()
            .filter_map(|s| match s {
                Step::RegisterProvider(p) => Some(*p),
                _ => None,
            })
            .collect();
        assert_eq!(
            providers,
            [
                Provider::MemoryService,
                Provider::CraftToolkit,
                Provider::Projects,
                Provider::Knowledge,
                Provider::OrdinHost
            ]
        );
        let adapters: Vec<_> = steps
            .iter()
            .filter(|s| matches!(s, Step::RegisterAdapter(_)))
            .collect();
        assert_eq!(adapters, [&Step::RegisterAdapter(Adapter::Claude)]);
        // usage.set_harnesses with the Claude Local Adapter only, right after row 3.32.
        let claude = position(&steps, Step::Construct(Row::ClaudeAdapter));
        assert_eq!(steps[claude + 2], Step::SetHarnesses);
        assert_eq!(
            steps.iter().filter(|s| **s == Step::SetHarnesses).count(),
            1
        );
        // bind_ref_resolver once, after row 3.43 and before McpFront.start.
        assert_eq!(
            steps
                .iter()
                .filter(|s| **s == Step::BindRefResolver)
                .count(),
            1
        );
        let bind = position(&steps, Step::BindRefResolver);
        assert!(bind > position(&steps, Step::Construct(Row::Setup)));
        assert_eq!(bind + 1, start);
        assert!(start < position(&steps, Step::Construct(Row::ServiceApi)));
        assert_eq!(bundled_program_names(), ["ordin-hook"]);
        assert!(features().is_empty());
    }

    #[test]
    fn the_catalogue_is_bound_as_the_part_source_right_after_it_is_built() {
        let steps = start_steps();
        let catalogue = position(&steps, Step::Construct(Row::Catalogue));
        assert_eq!(steps[catalogue + 1], Step::BindPartSource);
        assert!(position(&steps, Step::Construct(Row::Resolver)) < catalogue);
        assert!(position(&steps, Step::Construct(Row::Checker)) < catalogue);
    }

    #[test]
    fn every_provider_registers_before_the_front_starts_and_after_the_front_exists() {
        let steps = start_steps();
        let front = position(&steps, Step::Construct(Row::McpFront));
        let start = position(&steps, Step::StartMcpFront);
        for (at, step) in steps.iter().enumerate() {
            if matches!(step, Step::RegisterProvider(_) | Step::BindBlockStore) {
                assert!(front < at && at < start, "{step:?}");
            }
        }
        assert_eq!(
            steps.iter().filter(|s| **s == Step::StartMcpFront).count(),
            1
        );
    }

    #[test]
    fn the_command_bridge_is_built_last_after_the_service_api() {
        let steps = start_steps();
        assert_eq!(steps.last(), Some(&Step::Construct(Row::CommandBridge)));
        assert_eq!(steps[steps.len() - 2], Step::Construct(Row::ServiceApi));
    }

    #[test]
    fn the_updater_is_registered_right_after_the_setup_assistant() {
        let steps = start_steps();
        let setup = position(&steps, Step::Construct(Row::Setup));
        assert_eq!(steps[setup + 1], Step::RegisterUpdater);
    }

    // OrdinDesktop.md amendment January 2026 (ADR-114).
    #[cfg(all(
        feature = "sandbox",
        feature = "dictation",
        feature = "copilot",
        feature = "team-knowledge",
        feature = "authoring",
        feature = "handover",
        not(feature = "hosted-agents"),
        not(feature = "knowledge-agents"),
        not(feature = "telegram")
    ))]
    #[test]
    fn happy_path_a_build_with_the_team_switches_adds_the_team_modules() {
        let built = numbers(&rows());
        for present in [
            "3.11", "3.16", "3.27", "3.33", "3.39", "3.39a", "3.41a", "3.44", "3.45",
        ] {
            assert!(built.contains(&present), "{present}");
        }
        for absent in ["3.10", "3.13a", "3.46", "3.47"] {
            assert!(!built.contains(&absent), "{absent}");
        }
        let steps = start_steps();
        let providers = steps
            .iter()
            .filter(|s| matches!(s, Step::RegisterProvider(_)))
            .count();
        assert_eq!(providers, 9);
        let adapters = steps
            .iter()
            .filter(|s| matches!(s, Step::RegisterAdapter(_)))
            .count();
        assert_eq!(adapters, 3);
    }

    #[cfg(all(
        feature = "sandbox",
        feature = "mail",
        feature = "slack",
        feature = "authoring",
        feature = "telegram"
    ))]
    #[test]
    fn a_build_with_every_provider_switch_registers_ten_providers() {
        let steps = start_steps();
        let start = position(&steps, Step::StartMcpFront);
        let providers = steps[..start]
            .iter()
            .filter(|s| matches!(s, Step::RegisterProvider(_)))
            .count();
        assert_eq!(providers, 10);
        // bind_ref_resolver right after row 3.47, as before.
        let telegram = position(&steps, Step::Construct(Row::TelegramService));
        assert_eq!(
            steps[telegram + 1],
            Step::RegisterProvider(Provider::TelegramService)
        );
        assert_eq!(steps[telegram + 2], Step::BindRefResolver);
    }
}
