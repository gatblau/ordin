//! The migration steps of this module (*Data Model*): §A, the memory component's §B steps and the
//! additive step of each amendment, each under a number fixed in the tables below (MEM-MIG-1).

use std::sync::OnceLock;

use memory::store::sql::{SqlDialect, SqlStorage};
use ordin_journal::MigrationStep;

/// Where a step's SQL comes from.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// This service's own step: (description, SQL).
    Own(&'static str, &'static str),
    /// The memory component's step with this label in `SqlStorage::migrations`; its description
    /// is the label (MEM-MIG-2).
    Component(&'static str),
}

// A step's number never changes once written and is never reused. A new step, this service's own
// or the component's, takes the number one above the highest here and goes at the end, with the
// same number on both engines (MEM-MIG-4).
// spec: ordin-desktop/components/MemoryService#MEM-MIG-1
// spec: ordin-desktop/components/MemoryService#MEM-MIG-3
const SQLITE: [(i64, Source); 15] = [
    (
        1,
        Source::Own("memory_fact_refs", include_str!("sqlite_v1.sql")),
    ),
    (2, Source::Component("0001_init")),
    (3, Source::Component("0007_fact_collection")),
    (4, Source::Component("0008_graph")),
    (5, Source::Component("0009_job_fact")),
    (6, Source::Component("0010_link_author")),
    (
        7,
        Source::Own("team knowledge columns", include_str!("sqlite_team.sql")),
    ),
    (
        8,
        Source::Own("read folders", include_str!("sqlite_read_folders.sql")),
    ),
    (
        9,
        Source::Own("agentic memory", include_str!("sqlite_agentic.sql")),
    ),
    (
        10,
        Source::Own(
            "trusted pages and knowledge agents",
            include_str!("sqlite_trusted.sql"),
        ),
    ),
    (
        11,
        Source::Own(
            "versions of other sources",
            include_str!("sqlite_versions.sql"),
        ),
    ),
    (
        12,
        Source::Own("confirmed citations", include_str!("sqlite_citations.sql")),
    ),
    // spec: ordin-desktop/components/MemoryService#M-10
    (13, Source::Component("0011_triples")),
    (
        14,
        Source::Own("facts as triples", include_str!("sqlite_triples.sql")),
    ),
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-1
    (
        15,
        Source::Own("topics of things", include_str!("sqlite_topics.sql")),
    ),
];

// spec: ordin-desktop/components/MemoryService#MEM-MIG-1
// spec: ordin-desktop/components/MemoryService#MEM-MIG-3
const POSTGRES: [(i64, Source); 15] = [
    (
        1,
        Source::Own("memory_fact_refs", include_str!("postgres_v1.sql")),
    ),
    (2, Source::Component("0001_init")),
    (3, Source::Component("0007_fact_collection")),
    (4, Source::Component("0008_graph")),
    (5, Source::Component("0009_job_fact")),
    (6, Source::Component("0010_link_author")),
    (
        7,
        Source::Own("team knowledge columns", include_str!("postgres_team.sql")),
    ),
    (
        8,
        Source::Own("read folders", include_str!("postgres_read_folders.sql")),
    ),
    (
        9,
        Source::Own("agentic memory", include_str!("postgres_agentic.sql")),
    ),
    (
        10,
        Source::Own(
            "trusted pages and knowledge agents",
            include_str!("postgres_trusted.sql"),
        ),
    ),
    (
        11,
        Source::Own(
            "versions of other sources",
            include_str!("postgres_versions.sql"),
        ),
    ),
    (
        12,
        Source::Own(
            "confirmed citations",
            include_str!("postgres_citations.sql"),
        ),
    ),
    // spec: ordin-desktop/components/MemoryService#M-10
    (13, Source::Component("0011_triples")),
    (
        14,
        Source::Own("facts as triples", include_str!("postgres_triples.sql")),
    ),
    // spec: ordin-desktop/components/MemoryService#MEM-TOP-1
    (
        15,
        Source::Own("topics of things", include_str!("postgres_topics.sql")),
    ),
];

/// The steps of `table` in its order, a component step's SQL found by its label. A row whose label
/// the component no longer has is left out, leaving a gap; a component step without a row is given
/// the number 0. Either breaks the run 1, 2, 3, …, so the Journal's registry refuses the module.
// spec: ordin-desktop/components/MemoryService#MEM-MIG-2
// spec: ordin-desktop/components/MemoryService#MEM-MIG-5
fn steps(
    table: &[(i64, Source)],
    component: &'static [(&'static str, &'static str)],
) -> Vec<MigrationStep> {
    let mut steps = Vec::with_capacity(table.len() + 1);
    for (number, source) in table {
        match *source {
            Source::Own(description, sql) => steps.push((*number, description, sql)),
            Source::Component(label) => {
                if let Some((label, sql)) = component.iter().find(|(name, _)| *name == label) {
                    steps.push((*number, *label, *sql));
                }
            }
        }
    }
    for (label, sql) in component {
        let numbered = table
            .iter()
            .any(|(_, source)| matches!(source, Source::Component(name) if name == label));
        if !numbered {
            steps.push((0, *label, *sql));
        }
    }
    steps
}

/// The SQLite steps, built once: the component's pairs are read at run time.
pub(crate) fn sqlite_steps() -> &'static [MigrationStep] {
    static STEPS: OnceLock<Vec<MigrationStep>> = OnceLock::new();
    STEPS.get_or_init(|| steps(&SQLITE, SqlStorage::migrations(SqlDialect::Sqlite)))
}

/// The PostgreSQL steps.
pub(crate) fn postgres_steps() -> &'static [MigrationStep] {
    static STEPS: OnceLock<Vec<MigrationStep>> = OnceLock::new();
    STEPS.get_or_init(|| steps(&POSTGRES, SqlStorage::migrations(SqlDialect::Postgres)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_journal::{MigrationRegistry, ModuleMigrations};

    /// MEM-MIG-3 as literals. Change a line here only to add a step at the end.
    const PINNED: [(i64, &str); 15] = [
        (1, "memory_fact_refs"),
        (2, "0001_init"),
        (3, "0007_fact_collection"),
        (4, "0008_graph"),
        (5, "0009_job_fact"),
        (6, "0010_link_author"),
        (7, "team knowledge columns"),
        (8, "read folders"),
        (9, "agentic memory"),
        (10, "trusted pages and knowledge agents"),
        (11, "versions of other sources"),
        (12, "confirmed citations"),
        (13, "0011_triples"),
        (14, "facts as triples"),
        (15, "topics of things"),
    ];

    fn numbers(steps: &[MigrationStep]) -> Vec<(i64, &'static str)> {
        steps.iter().map(|(n, d, _)| (*n, *d)).collect()
    }

    fn register(sqlite: Vec<MigrationStep>) -> bool {
        let mut registry = MigrationRegistry::new();
        registry
            .register(ModuleMigrations {
                module: crate::MODULE_NAME,
                sqlite: sqlite.leak(),
                postgres: &[],
            })
            .is_ok()
    }

    // spec: ordin-desktop/components/MemoryService#MEM-MIG-6
    #[test]
    fn every_step_keeps_its_number_on_both_engines() {
        assert_eq!(numbers(sqlite_steps()), PINNED, "SQLite");
        assert_eq!(numbers(postgres_steps()), PINNED, "PostgreSQL");
    }

    // spec: ordin-desktop/components/MemoryService#MEM-MIG-6
    #[test]
    fn every_component_step_has_a_row_and_its_own_sql() {
        for (dialect, built) in [
            (SqlDialect::Sqlite, sqlite_steps()),
            (SqlDialect::Postgres, postgres_steps()),
        ] {
            for (label, sql) in SqlStorage::migrations(dialect) {
                let found: Vec<_> = built.iter().filter(|(_, d, _)| d == label).collect();
                assert_eq!(found.len(), 1, "{dialect:?} {label}");
                assert!(found[0].0 >= 1, "{dialect:?} {label} has no row");
                assert_eq!(found[0].2, *sql, "{dialect:?} {label}");
            }
        }
        assert!(register(sqlite_steps().to_vec()));
        assert!(register(postgres_steps().to_vec()));
    }

    // spec: ordin-desktop/components/MemoryService#MEM-MIG-2
    #[test]
    fn a_reordered_component_list_changes_no_number() {
        let reversed: Vec<(&'static str, &'static str)> =
            SqlStorage::migrations(SqlDialect::Sqlite)
                .iter()
                .rev()
                .copied()
                .collect();
        let built = steps(&SQLITE, reversed.leak());
        assert_eq!(numbers(&built), PINNED);
        assert_eq!(built, sqlite_steps());
    }

    // spec: ordin-desktop/components/MemoryService#MEM-MIG-5
    #[test]
    fn a_component_step_without_a_row_is_refused_at_registration() {
        let mut more = SqlStorage::migrations(SqlDialect::Sqlite).to_vec();
        more.push(("0011_not_numbered", "SELECT 1;"));
        let built = steps(&SQLITE, more.leak());
        assert_eq!(
            built.last().map(|s| (s.0, s.1)),
            Some((0, "0011_not_numbered"))
        );
        assert!(!register(built));
    }

    // spec: ordin-desktop/components/MemoryService#MEM-MIG-5
    #[test]
    fn a_row_whose_component_step_is_gone_is_refused_at_registration() {
        let fewer: Vec<(&'static str, &'static str)> = SqlStorage::migrations(SqlDialect::Postgres)
            .iter()
            .filter(|(label, _)| *label != "0008_graph")
            .copied()
            .collect();
        let built = steps(&POSTGRES, fewer.leak());
        assert!(!built.iter().any(|(n, _, _)| *n == 4));
        assert!(!register(built));
    }
}
