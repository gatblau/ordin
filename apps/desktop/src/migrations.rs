//! Row 3.1's registrations (OrdinDesktop.md *Program-side implementations and calls*,
//! `MigrationRegistry::register`; DatabaseMigrations.md *Modules that register migrations*): the
//! Journal's own steps first, then each table owner of this build in the order of step 3.

use ordin_journal::{JournalError, MigrationRegistry, ModuleMigrations};

/// A phase-1 owner's `MODULE_NAME` + `SQLITE_MIGRATIONS`, wrapped with an empty `postgres`.
fn phase_one(
    module: &'static str,
    sqlite: &'static [ordin_journal::MigrationStep],
) -> ModuleMigrations {
    ModuleMigrations {
        module,
        sqlite,
        postgres: &[],
    }
}

/// Every registration of this build, in registration order.
///
/// The crates behind the switches `mail`, `slack`, `authoring`, `handover` and `telegram` export no
/// `migrations()` yet (their Playbook steps are not built), so a build with those switches registers
/// nothing for them; `team-knowledge` registers module `team_knowledge` (Step 74's first build).
pub fn registrations() -> Vec<ModuleMigrations> {
    #[allow(unused_mut)]
    let mut all = vec![
        ordin_journal::MIGRATIONS,
        phase_one(
            ordin_organisation_profile::MODULE_NAME,
            ordin_organisation_profile::SQLITE_MIGRATIONS,
        ),
        phase_one(
            ordin_identity::MODULE_NAME,
            ordin_identity::SQLITE_MIGRATIONS,
        ),
        ordin_approval::ApprovalService::migrations(),
        ordin_localisation::migrations(),
        ordin_agent_folders::migrations(),
        ordin_sharepoint::migrations(),
        ordin_usage_planner::UsagePlanner::migrations(),
        ordin_server_connector::ServerConnector::migrations(),
        // The crate is in every build (DSK-UP-1); the `knowledge-agents` switch gates construction.
        ordin_knowledge_query_client::migrations(),
        ordin_rules::RulesEngine::migrations(),
        ordin_brand::migrations(),
        ordin_checker::migrations(),
        ordin_session_supervisor::SessionSupervisor::migrations(),
        ordin_files::FileService::migrations(),
        ordin_hook_relay::HookRelay::migrations(),
        ordin_crew_orchestrator::CrewOrchestrator::migrations(),
        ordin_memory_service::MemoryService::migrations(),
        ordin_craft_toolkit::CraftToolkit::migrations(),
        ordin_confluence::ConfluenceAdapter::migrations(),
        ordin_projects::ProjectService::migrations(),
        ordin_workspaces::migrations(),
        ordin_catalogue::migrations(),
        ordin_push::PushService::migrations(),
        ordin_knowledge::KnowledgeService::migrations(),
        ordin_package_catalogue::migrations(),
        ordin_host::OrdinHost::migrations(),
        ordin_home_search::HomeSearch::migrations(),
        ordin_setup::migrations(),
    ];
    #[cfg(feature = "team-knowledge")]
    all.push(ordin_team_knowledge::TeamKnowledgeService::migrations());
    all
}

/// Registers every module of this build once (DatabaseMigrations.md *Internal Logic* 1).
pub fn register_all(registry: &mut MigrationRegistry) -> Result<(), JournalError> {
    for migrations in registrations() {
        registry.register(migrations)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;
    use ordin_journal::{ApplyMode, Datastore, DatastoreConfig};
    use std::collections::BTreeSet;

    #[test]
    fn every_module_of_the_desktop_is_registered_once() {
        let modules: Vec<_> = registrations().iter().map(|m| m.module).collect();
        assert_eq!(modules.first(), Some(&"journal"));
        let unique: BTreeSet<_> = modules.iter().collect();
        assert_eq!(unique.len(), modules.len(), "no module twice: {modules:?}");
        let mut registry = MigrationRegistry::new();
        register_all(&mut registry).expect("each module once, versions 1, 2, 3 …");
        // A second registration of any module is refused (JRNL_MIGRATION_DUPLICATE).
        assert!(registry.register(ordin_journal::MIGRATIONS).is_err());
    }

    #[tokio::test]
    async fn the_registrations_apply_to_an_empty_journal_database() {
        let dir = TempDir::new("migrations");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.path().join("journal.db"),
            backup_dir: dir.path().join("backups"),
        })
        .await
        .expect("open");
        let mut registry = MigrationRegistry::new();
        register_all(&mut registry).expect("register");
        let report = registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply");
        let applied: BTreeSet<_> = report.applied.iter().map(|(m, _)| m.as_str()).collect();
        for module in registrations().iter().map(|m| m.module) {
            assert!(applied.contains(module), "{module} applied");
        }
        // A second start applies nothing.
        let again = registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply again");
        assert!(again.applied.is_empty());
        store.close().await;
    }
}
