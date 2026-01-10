//! The one migration registry of a program (JournalAndArtefactStore.md *Migration registry* and
//! Internal Logic 2; DatabaseMigrations.md, which lands here).

use std::path::{Path, PathBuf};

use ordin_types::num::Sha256Hex;
use ordin_types::time::Timestamp;
use sha2::{Digest, Sha256};

use crate::datastore::{create_private_dir, set_private_file, Datastore, Engine};
use crate::db::{self, Arg, Tx};
use crate::error::JournalError;

/// (version, description, SQL). Versions start at 1 and increase by exactly 1 within a module.
// spec: ordin-desktop/components/JournalAndArtefactStore#SA-RECON-01
pub type MigrationStep = (i64, &'static str, &'static str);

#[derive(Debug, Clone, Copy)]
pub struct ModuleMigrations {
    /// `[a-z][a-z0-9_]{1,40}`: the crate name without `ordin-`, hyphens as underscores
    /// (for example `journal`, `organisation_profile`, `identity`).
    pub module: &'static str,
    pub sqlite: &'static [MigrationStep],
    /// Engine rule (DatabaseMigrations.md M-2): a module that runs in the agent profile's scaled
    /// shape (2B-3 Programs column includes A) supplies the same versions here as in `sqlite`;
    /// empty for a module that runs only on D or P, or whose data the scaled shape does not share.
    pub postgres: &'static [MigrationStep],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    /// Desktop, personal server, single shape, and the scaled shape's schema command.
    ApplyPending,
    /// Scaled-shape instances: refuse to start when anything is pending (ADR-098 item 5).
    CheckOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// (module, version) applied by this call, in order.
    pub applied: Vec<(String, i64)>,
    pub backup_file: Option<std::path::PathBuf>,
}

/// This crate's own migrations; the program registers them first.
pub const MIGRATIONS: ModuleMigrations = ModuleMigrations {
    module: "journal",
    sqlite: &[(
        1,
        "create journal tables",
        include_str!("sql/sqlite_v1.sql"),
    )],
    postgres: &[(
        1,
        "create journal tables",
        include_str!("sql/postgres_v1.sql"),
    )],
};

/// The bookkeeping table, created by `apply` before any module's steps (Data Model).
const SCHEMA_MIGRATIONS_SQLITE: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    module       TEXT    NOT NULL CHECK (length(module) BETWEEN 2 AND 41),
    version      INTEGER NOT NULL CHECK (version >= 1),
    description  TEXT    NOT NULL,
    checksum     TEXT    NOT NULL CHECK (length(checksum) = 64),
    applied_at   TEXT    NOT NULL,
    PRIMARY KEY (module, version)
)";

const SCHEMA_MIGRATIONS_POSTGRES: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
    module       text        NOT NULL CHECK (char_length(module) BETWEEN 2 AND 41),
    version      bigint      NOT NULL CHECK (version >= 1),
    description  text        NOT NULL,
    checksum     text        NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    applied_at   timestamptz NOT NULL,
    PRIMARY KEY (module, version)
)";

/// Two schema commands never run together (Internal Logic 2.8; constant chosen there).
const MIGRATION_LOCK_KEY: i64 = 7_243_100_001;
/// Backups kept beside the newest (Configuration and constants: newest 2 kept).
const BACKUPS_KEPT: usize = 2;

/// The modules of a program, in registration order.
pub struct MigrationRegistry {
    modules: Vec<ModuleMigrations>,
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRegistry {
    pub fn new() -> Self {
        MigrationRegistry {
            modules: Vec::new(),
        }
    }

    /// Fails with JRNL_MIGRATION_DUPLICATE when the module is already registered or its
    /// versions are not 1, 2, 3, … without gaps.
    pub fn register(&mut self, migrations: ModuleMigrations) -> Result<(), JournalError> {
        let refused = || {
            tracing::error!(
                component = "journal",
                module = module_for_log(migrations.module),
                code = "JRNL_MIGRATION_DUPLICATE",
                "journal.migrate.refused"
            );
            JournalError::MigrationDuplicate {
                module: migrations.module.to_owned(),
            }
        };
        let already = self.modules.iter().any(|m| m.module == migrations.module);
        if already
            || !valid_module_name(migrations.module)
            || !consecutive(migrations.sqlite)
            || !consecutive(migrations.postgres)
        {
            return Err(refused());
        }
        self.modules.push(migrations);
        Ok(())
    }

    pub async fn apply(
        &self,
        store: &Datastore,
        mode: ApplyMode,
    ) -> Result<MigrationReport, JournalError> {
        let engine = store.engine();
        // Step 2: the bookkeeping table, outside the migration transaction.
        let ddl = match engine {
            Engine::Sqlite => SCHEMA_MIGRATIONS_SQLITE,
            Engine::Postgres => SCHEMA_MIGRATIONS_POSTGRES,
        };
        db::execute(store, ddl, &[]).await?;

        // Step 3: every applied row is checked against what this program knows.
        let rows = db::fetch_all(
            store,
            "SELECT module, version, checksum FROM schema_migrations ORDER BY module, version",
            &[],
        )
        .await?;
        let mut applied: Vec<(String, i64)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let module = row.text(0)?;
            let version = row.int(1)?;
            let checksum = row.text(2)?;
            self.check_applied(engine, &module, version, &checksum)?;
            applied.push((module, version));
        }

        // Step 4: the pending list, in registration order.
        let mut pending: Vec<(&'static str, MigrationStep)> = Vec::new();
        for module in &self.modules {
            let highest = applied
                .iter()
                .filter(|(name, _)| name == module.module)
                .map(|(_, version)| *version)
                .max()
                .unwrap_or(0);
            for step in steps_for(module, engine) {
                if step.0 > highest {
                    pending.push((module.module, *step));
                }
            }
        }

        // Steps 5 and 6.
        if pending.is_empty() {
            return Ok(MigrationReport {
                applied: Vec::new(),
                backup_file: None,
            });
        }
        let pending_list: Vec<(String, i64)> = pending
            .iter()
            .map(|(module, step)| ((*module).to_owned(), step.0))
            .collect();
        if mode == ApplyMode::CheckOnly {
            tracing::error!(
                component = "journal",
                code = "JRNL_SCHEMA_BEHIND",
                pending = pending_list.len(),
                "journal.migrate.refused"
            );
            return Err(JournalError::SchemaBehind {
                pending: pending_list,
            });
        }

        // Step 7: the backup copy (SQLite only). A database with no step applied yet holds no saved
        // work, so the first start writes none (Gherkin *First start creates the database*).
        let backup_file = match store {
            Datastore::Sqlite(pool) if !applied.is_empty() => {
                Some(write_backup(pool, store.backup_dir()).await?)
            }
            _ => None,
        };

        // Step 8: every pending step in one transaction.
        let mut tx = Tx::begin(store).await?;
        if engine == Engine::Postgres {
            tx.execute(
                "SELECT pg_advisory_xact_lock($1)",
                &[Arg::Int(MIGRATION_LOCK_KEY)],
            )
            .await?;
        }
        for (module, (version, description, sql)) in &pending {
            let failed = || {
                tracing::error!(
                    component = "journal",
                    module = *module,
                    version = *version,
                    "journal.migrate.failed"
                );
                JournalError::MigrationFailed {
                    module: (*module).to_owned(),
                    version: *version,
                }
            };
            let ran = match &mut tx {
                Tx::Sqlite(t) => sqlx::raw_sql(*sql).execute(&mut **t).await.map(|_| ()),
                Tx::Pg(t) => sqlx::raw_sql(*sql).execute(&mut **t).await.map(|_| ()),
            };
            ran.map_err(|_| failed())?;
            tx.execute(
                "INSERT INTO schema_migrations (module, version, description, checksum, applied_at) \
                 VALUES ($1, $2, $3, $4, $5{ts})",
                &[
                    Arg::Text((*module).to_owned()),
                    Arg::Int(*version),
                    Arg::Text((*description).to_owned()),
                    Arg::Text(checksum(sql)),
                    Arg::Text(Timestamp::now().to_wire()),
                ],
            )
            .await
            .map_err(|_| failed())?;
        }
        tx.commit().await.map_err(|_| {
            let (module, (version, _, _)) = pending[pending.len() - 1];
            JournalError::MigrationFailed {
                module: module.to_owned(),
                version,
            }
        })?;

        // Step 9.
        for (module, version) in &pending_list {
            tracing::info!(
                component = "journal",
                module = module.as_str(),
                version = *version,
                "journal.migrate.applied"
            );
        }
        Ok(MigrationReport {
            applied: pending_list,
            backup_file,
        })
    }

    /// Internal Logic 2.3 for one row of `schema_migrations`.
    fn check_applied(
        &self,
        engine: Engine,
        module: &str,
        version: i64,
        stored_checksum: &str,
    ) -> Result<(), JournalError> {
        let refuse = |code: &'static str, error: JournalError| {
            tracing::error!(
                component = "journal",
                module = module_for_log(module),
                version,
                code,
                "journal.migrate.refused"
            );
            Err(error)
        };
        let Some(registered) = self.modules.iter().find(|m| m.module == module) else {
            return refuse(
                "JRNL_SCHEMA_NEWER",
                JournalError::SchemaNewer {
                    module: module.to_owned(),
                    found: version,
                    known: 0,
                },
            );
        };
        let steps = steps_for(registered, engine);
        let known = steps.iter().map(|s| s.0).max().unwrap_or(0);
        let Some(step) = steps
            .iter()
            .find(|s| s.0 == version)
            .filter(|_| version <= known)
        else {
            return refuse(
                "JRNL_SCHEMA_NEWER",
                JournalError::SchemaNewer {
                    module: module.to_owned(),
                    found: version,
                    known,
                },
            );
        };
        if checksum(step.2) != stored_checksum {
            return refuse(
                "JRNL_MIGRATION_CHANGED",
                JournalError::MigrationChanged {
                    module: module.to_owned(),
                    version,
                },
            );
        }
        Ok(())
    }
}

/// The steps of the engine the datastore runs on.
fn steps_for(module: &ModuleMigrations, engine: Engine) -> &'static [MigrationStep] {
    match engine {
        Engine::Sqlite => module.sqlite,
        Engine::Postgres => module.postgres,
    }
}

/// `[a-z][a-z0-9_]{1,40}`.
fn valid_module_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=41).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}

/// A module name goes to a log line only when it has the declared form; otherwise `unknown`
/// (Logging.md R-7: a name taken from input is logged only after it matched).
fn module_for_log(name: &str) -> &str {
    if valid_module_name(name) {
        name
    } else {
        "unknown"
    }
}

/// Versions 1, 2, 3, … without gaps (an empty slice is allowed).
fn consecutive(steps: &[MigrationStep]) -> bool {
    steps
        .iter()
        .enumerate()
        .all(|(i, step)| i64::try_from(i + 1).is_ok_and(|expected| step.0 == expected))
}

/// SHA-256 hex of a step's SQL text for the current engine.
fn checksum(sql: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(sql.as_bytes()).into();
    Sha256Hex::from_digest(&digest).0
}

/// Internal Logic 2.7: `VACUUM INTO` a consistent copy, then keep the newest two.
async fn write_backup(
    pool: &sqlx::SqlitePool,
    backup_dir: Option<PathBuf>,
) -> Result<PathBuf, JournalError> {
    let failed = |reason: &'static str| {
        tracing::error!(
            component = "journal",
            code = "JRNL_BACKUP_FAILED",
            reason,
            "journal.backup.failed"
        );
        JournalError::BackupFailed
    };
    let dir = backup_dir.ok_or_else(|| failed("folder"))?;
    if !dir.is_dir() {
        create_private_dir(&dir).map_err(|_| failed("folder"))?;
    }
    let stamp = Timestamp::now().0.format("%Y%m%dT%H%M%SZ").to_string();
    let file_name = format!("journal-{stamp}.db");
    let target = dir.join(&file_name);
    // SPEC-GAP-2: two backups in the same second share one name, and `VACUUM INTO` refuses an
    // existing file. The copy is written under a temporary name and renamed over the earlier one,
    // so the newest copy taken before this call's migration is the one kept.
    let partial = dir.join(format!(".{file_name}.partial"));
    let _ = std::fs::remove_file(&partial);
    let partial_text = partial.to_str().ok_or_else(|| failed("folder"))?.to_owned();
    sqlx::query("VACUUM INTO $1")
        .bind(partial_text)
        .execute(pool)
        .await
        .map_err(|_| failed("copy"))?;
    set_private_file(&partial).map_err(|_| failed("permissions"))?;
    std::fs::rename(&partial, &target).map_err(|_| failed("copy"))?;
    prune_backups(&dir);
    tracing::info!(
        component = "journal",
        file_name = file_name.as_str(),
        "journal.backup.written"
    );
    Ok(target)
}

/// Deletes older backups so the newest two remain. The names sort by time.
fn prune_backups(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("journal-") && n.ends_with(".db"))
        .collect();
    names.sort();
    let surplus = names.len().saturating_sub(BACKUPS_KEPT);
    for name in names.iter().take(surplus) {
        // A copy that cannot be removed stays; the next migration tries again.
        let _ = std::fs::remove_file(dir.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::CodedError;

    const STEP: &[MigrationStep] = &[(1, "create", "CREATE TABLE t (x INTEGER)")];

    /// Module names of the DatabaseMigrations.md table *Modules that register migrations*, with
    /// the programs (2B-3) each runs in. The server program's own `server` module is P and A.
    const MODULES: &[(&str, &str)] = &[
        ("journal", "DPA"),
        ("organisation_profile", "DP"),
        ("identity", "DPA"),
        ("approval", "DPA"),
        ("localisation", "DPA"),
        ("agent_folders", "DPA"),
        ("usage_planner", "DPA"),
        ("knowledge_query_client", "DPA"),
        ("run_queue", "A"),
        ("settings_store", "A"),
        ("sharepoint", "DP"),
        ("server_connector", "D"),
        ("rules", "DPA"),
        ("brand", "D"),
        ("checker", "DPA"),
        ("session_supervisor", "DPA"),
        ("duty_timer", "A"),
        ("remote_api", "P"),
        ("files", "DPA"),
        ("hook_relay", "DPA"),
        ("crew_orchestrator", "DP"),
        ("memory_service", "DPA"),
        ("craft", "DP"),
        ("confluence", "D"),
        ("projects", "DP"),
        ("workspaces", "DP"),
        ("catalogue", "DPA"),
        ("push", "DP"),
        ("knowledge_query_server", "A"),
        ("knowledge", "DPA"),
        ("mail", "DP"),
        ("slack_service", "DP"),
        ("team_knowledge", "DP"),
        ("package_catalogue", "DP"),
        ("ordin_host", "DP"),
        ("home_search", "DP"),
        ("setup", "DP"),
        ("retention", "A"),
        ("knowledge_reader", "A"),
        ("authoring", "DP"),
        ("channel_gateway", "PA"),
        ("handover", "DP"),
        ("job_runner", "A"),
        ("telegram_adapter", "DP"),
        ("slack_channel_adapter", "PA"),
        ("triggers", "P"),
        ("telegram_service", "DP"),
        ("server", "PA"),
    ];

    fn module(name: &'static str) -> ModuleMigrations {
        ModuleMigrations {
            module: name,
            sqlite: STEP,
            postgres: &[],
        }
    }

    // Registration unit test (Playbook Step 5 gate; DatabaseMigrations.md Internal Logic 1): every
    // module of each program registers once, and a second registration is JRNL_MIGRATION_DUPLICATE.
    #[test]
    fn every_module_of_each_program_registers_once() {
        assert_eq!(
            MODULES.len(),
            48,
            "48 registrations in all (DatabaseMigrations.md)"
        );
        for program in ['D', 'P', 'A'] {
            let mut registry = MigrationRegistry::new();
            registry
                .register(MIGRATIONS)
                .expect("the journal registers first");
            for (name, programs) in MODULES.iter().filter(|(n, _)| *n != "journal") {
                if programs.contains(program) {
                    registry
                        .register(module(name))
                        .unwrap_or_else(|e| panic!("{program}: {name} refused with {}", e.code()));
                }
            }
            for (name, programs) in MODULES {
                if programs.contains(program) {
                    let second = if *name == "journal" {
                        registry.register(MIGRATIONS)
                    } else {
                        registry.register(module(name))
                    };
                    match second {
                        Err(JournalError::MigrationDuplicate { module }) => {
                            assert_eq!(module, *name)
                        }
                        other => panic!("{program}: second {name} gave {other:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn versions_must_run_without_gaps_and_names_must_match_the_form() {
        let mut registry = MigrationRegistry::new();
        let gap: &'static [MigrationStep] = &[(1, "a", "SELECT 1"), (3, "c", "SELECT 1")];
        let zero: &'static [MigrationStep] = &[(0, "a", "SELECT 1")];
        for bad in [
            ModuleMigrations {
                module: "gappy",
                sqlite: gap,
                postgres: &[],
            },
            ModuleMigrations {
                module: "zero",
                sqlite: zero,
                postgres: &[],
            },
            ModuleMigrations {
                module: "pg_gap",
                sqlite: STEP,
                postgres: gap,
            },
            ModuleMigrations {
                module: "Upper",
                sqlite: STEP,
                postgres: &[],
            },
            ModuleMigrations {
                module: "x",
                sqlite: STEP,
                postgres: &[],
            },
            ModuleMigrations {
                module: "has-hyphen",
                sqlite: STEP,
                postgres: &[],
            },
        ] {
            let refusal = registry.register(bad).unwrap_err();
            assert_eq!(refusal.code(), "JRNL_MIGRATION_DUPLICATE", "{}", bad.module);
            assert!(!refusal.retryable());
        }
        // A module with no step at all is accepted (a module with no tables in one engine).
        registry
            .register(ModuleMigrations {
                module: "empty",
                sqlite: &[],
                postgres: &[],
            })
            .unwrap();
    }

    #[test]
    fn the_journal_registers_the_same_versions_on_both_engines() {
        assert_eq!(MIGRATIONS.module, "journal");
        let sqlite: Vec<i64> = MIGRATIONS.sqlite.iter().map(|s| s.0).collect();
        let postgres: Vec<i64> = MIGRATIONS.postgres.iter().map(|s| s.0).collect();
        assert_eq!(sqlite, postgres);
        assert_eq!(checksum("x").len(), 64);
    }
}
