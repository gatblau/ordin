//! X7 — Database migrations. A `Migrator` applies numbered, ordered `NNNN_<slug>.{up,down}.surql`
//! pairs idempotently per tenant namespace, recording applied versions in a `schema_migrations`
//! table. Tables start schemaless and tighten later (SA-MIG-01). Applies to a shared store are an
//! explicit user action; `dry_run` is the review artefact (sql-safety layer rule).

use serde_json::Value as Json;
use surrealdb::engine::any::Any;
use surrealdb::Surreal;

use crate::types::ports::StoreError;

/// One embedded migration: a zero-padded version, a slug, and its up/down SurrealQL text. The text is
/// compiled into the binary via `include_str!` so the running service is self-contained.
///
/// `pub(crate)` only because `render_up` takes one and is itself `pub(crate)` for SA-BENCH-RENDER-01;
/// the fields stay private, so nothing outside this module can construct or read a `Migration`.
pub(crate) struct Migration {
    version: u32,
    slug: &'static str,
    up: &'static str,
    down: &'static str,
}

/// The ordered, monotonic migration set. New migrations append here with the next version number.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        slug: "init",
        // The full greenfield schema (C1 store + audit, C2 queue, C4 quarantine, C7 maintenance_state,
        // C8 idempotency_record) squashed into one initial migration before first release.
        up: include_str!("../../migrations/0001_init.up.surql"),
        down: include_str!("../../migrations/0001_init.down.surql"),
    },
    Migration {
        version: 2,
        slug: "entity_element_index",
        // RISK-018: `fact_ent` indexed `entities` as a whole array and so could not serve
        // `$entity IN entities`. Appended rather than folded into 0001 because any namespace already
        // provisioned records version 1 and would never re-run an amended 0001 — editing it in place
        // would leave provisioned and fresh namespaces silently on different schemas.
        up: include_str!("../../migrations/0002_entity_element_index.up.surql"),
        down: include_str!("../../migrations/0002_entity_element_index.down.surql"),
    },
    Migration {
        version: 3,
        slug: "derived_from_index",
        // FU-025: `hard_delete`'s derived-collection query had no index over `derived_from` and
        // planned a full table scan. Adds one, in the `.*` element form 0002 established.
        up: include_str!("../../migrations/0003_derived_from_index.up.surql"),
        down: include_str!("../../migrations/0003_derived_from_index.down.surql"),
    },
    Migration {
        version: 4,
        slug: "consent_grants",
        // SA-GRANT-01: the `grant` table backing the fourth arm of the fact read filter. Additive —
        // it defines a new table and two indexes and alters nothing existing, so a namespace already
        // on version 3 gains the arm without its facts being touched.
        up: include_str!("../../migrations/0004_consent_grants.up.surql"),
        down: include_str!("../../migrations/0004_consent_grants.down.surql"),
    },
    Migration {
        version: 5,
        slug: "audit_authority",
        // SA-AUTHZ-04: two option<string> columns on `audit_log` (`client`, `fact_class`) so a
        // refused class write leaves a record naming who was refused and what they tried to assert.
        // Additive — `audit_log` is SCHEMAFULL, so without these the refusal could not be audited.
        up: include_str!("../../migrations/0005_audit_authority.up.surql"),
        down: include_str!("../../migrations/0005_audit_authority.down.surql"),
    },
    Migration {
        version: 6,
        slug: "quarantine_fact_class",
        // SA-AUTHZ-09: `fact_class` on the SCHEMAFULL `quarantine` table, so a held write records
        // what it claimed to be. Additive. The `fact` table needs no equivalent — it is SCHEMALESS.
        up: include_str!("../../migrations/0006_quarantine_fact_class.up.surql"),
        down: include_str!("../../migrations/0006_quarantine_fact_class.down.surql"),
    },
];

/// The database name used inside every tenant namespace (ADR-011: one namespace per tenant, one
/// database `recall` inside each).
pub const DB_NAME: &str = "memory";

/// Applies and reverses the numbered migration set against a per-tenant SurrealDB namespace.
pub struct Migrator<'a> {
    db: &'a Surreal<Any>,
    /// The embedding dimension substituted into the HNSW index DDL (`MEMORY_EMBED_DIM`, SA-EMBED-01).
    embed_dim: u32,
}

impl<'a> Migrator<'a> {
    /// Build a migrator bound to a live connection and the configured embedding dimension.
    pub fn new(db: &'a Surreal<Any>, embed_dim: u32) -> Self {
        Self { db, embed_dim }
    }

    /// Prefix `sql` with this tenant's `USE`, so the namespace travels in the SAME statement batch as
    /// the work (RISK-012). No connection session state is mutated, so a concurrent task cannot switch
    /// the namespace between selection and execution. Callers reading results must account for the
    /// `USE` occupying result slot 0 — the first caller statement is at index 1.
    ///
    /// DDL identifiers cannot be bound as query variables, so the tenant is interpolated; it passes
    /// `validate_tenant` first, which is the only gate (sql-safety layer rule).
    pub(crate) fn scoped(tenant: &str, sql: impl AsRef<str>) -> Result<String, StoreError> {
        Self::scoped_at(tenant, DB_NAME, sql)
    }

    /// The primitive form: bind `sql` to an explicit namespace and database. Carries no policy about
    /// which namespace a tenant belongs to — that is `types::placement::resolve`, reached through
    /// `Store::scoped`. Both identifiers are validated before interpolation (sql-safety layer rule);
    /// DDL identifiers cannot be bound as query variables.
    pub(crate) fn scoped_at(
        namespace: &str,
        database: &str,
        sql: impl AsRef<str>,
    ) -> Result<String, StoreError> {
        validate_tenant(namespace)?;
        validate_tenant(database)?;
        Ok(format!(
            "USE NS {namespace} DB {database}; {}",
            sql.as_ref()
        ))
    }

    /// Define the tenant namespace, its `recall` database, and the migration bookkeeping table,
    /// idempotently. Defines only — selects nothing, mutates no session state.
    async fn provision_namespace(&self, tenant: &str) -> Result<(), StoreError> {
        validate_tenant(tenant)?;
        let ddl = format!(
            "DEFINE NAMESPACE IF NOT EXISTS {tenant}; \
             USE NS {tenant}; \
             DEFINE DATABASE IF NOT EXISTS {DB_NAME}; \
             USE NS {tenant} DB {DB_NAME}; \
             {SCHEMA_MIGRATIONS_DDL}"
        );
        self.db
            .query(ddl)
            .await
            .map_err(map_db_err)?
            .check()
            .map_err(map_db_err)?;
        Ok(())
    }

    /// The highest applied migration version for a tenant (0 on a fresh namespace).
    pub async fn current_version(&self, tenant: &str) -> Result<u32, StoreError> {
        self.provision_namespace(tenant).await?;
        let mut resp = self
            .db
            .query(Self::scoped(
                tenant,
                "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
            )?)
            .await
            .map_err(map_db_err)?;
        // Slot 0 is the `USE`; the SELECT's rows are at index 1.
        let rows: Vec<Json> = resp
            .take(1)
            .map_err(|e| StoreError::Internal(format!("read schema_migrations: {e}")))?;
        let version = rows
            .first()
            .and_then(|r| r.get("version"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(0);
        Ok(version)
    }

    /// Apply every pending `up` migration in order, recording each version. Idempotent: a second call
    /// with nothing pending is a no-op. Returns the new current version.
    pub async fn migrate_up(&self, tenant: &str) -> Result<u32, StoreError> {
        let mut current = self.current_version(tenant).await?;
        let pending: Vec<&Migration> = MIGRATIONS.iter().filter(|m| m.version > current).collect();
        for m in pending {
            let stmts = self.render_up(m);
            self.db
                .query(Self::scoped(tenant, &stmts)?)
                .await
                .map_err(map_db_err)?
                .check()
                .map_err(map_db_err)?;
            self.db
                .query(Self::scoped(
                    tenant,
                    "CREATE schema_migrations SET version = $v, applied_at = time::now()",
                )?)
                .bind(("v", m.version as i64))
                .await
                .map_err(map_db_err)?
                .check()
                .map_err(map_db_err)?;
            tracing::info!(target: "memory", version = m.version, slug = m.slug, "migrate.applied");
            current = m.version;
        }
        Ok(current)
    }

    /// Return the statements that `migrate_up` WOULD run for the pending migrations, executing
    /// nothing. The review artefact before a shared-store apply (X7 Internal Logic 2).
    pub async fn dry_run(&self, tenant: &str) -> Result<Vec<String>, StoreError> {
        let current = self.current_version(tenant).await?;
        let mut out = Vec::new();
        for m in MIGRATIONS.iter().filter(|m| m.version > current) {
            out.push(self.render_up(m));
        }
        Ok(out)
    }

    /// Run the `down` pairs from the current version down to (and excluding) `to`. Destructive downs
    /// (the `0001` table drop) only ever target an empty store; this method does not guard population
    /// (the caller — an explicit user action — owns that decision per X7), but logs the intent.
    pub async fn migrate_down(&self, tenant: &str, to: u32) -> Result<(), StoreError> {
        let current = self.current_version(tenant).await?;
        for m in MIGRATIONS
            .iter()
            .rev()
            .filter(|m| m.version > to && m.version <= current)
        {
            tracing::warn!(target: "memory", version = m.version, slug = m.slug, "migrate.down");
            self.db
                .query(Self::scoped(tenant, m.down)?)
                .await
                .map_err(map_db_err)?
                .check()
                .map_err(map_db_err)?;
            self.db
                .query(Self::scoped(
                    tenant,
                    "DELETE schema_migrations WHERE version = $v",
                )?)
                .bind(("v", m.version as i64))
                .await
                .map_err(map_db_err)?
                .check()
                .map_err(map_db_err)?;
        }
        Ok(())
    }

    /// Render an `up` migration with the `<dim>` HNSW placeholder substituted by `MEMORY_EMBED_DIM`.
    ///
    /// `pub(crate)` for SA-BENCH-RENDER-01: C11's database-per-tenant leg reuses this renderer rather
    /// than re-reading `migrations/0001_init.up.surql` itself, so the two shapes cannot drift on the
    /// `<dim>` substitution. `pub(crate)` is the narrowest visibility that reaches `src/bench`; C1's
    /// public surface is unchanged (rule C12).
    pub(crate) fn render_up(&self, m: &Migration) -> String {
        m.up.replace("<dim>", &self.embed_dim.to_string())
    }
}

/// DDL for the migration bookkeeping table (X7 Data Model). Idempotent.
const SCHEMA_MIGRATIONS_DDL: &str = "\
DEFINE TABLE IF NOT EXISTS schema_migrations SCHEMAFULL; \
DEFINE FIELD IF NOT EXISTS version    ON schema_migrations TYPE int; \
DEFINE FIELD IF NOT EXISTS applied_at ON schema_migrations TYPE datetime; \
DEFINE INDEX IF NOT EXISTS sm_version ON schema_migrations FIELDS version UNIQUE;";

/// Reject a tenant id that is not a safe SurrealDB namespace identifier. Tenant ids originate from a
/// validated OIDC claim (C3); this is a defensive second check before interpolating into DDL where
/// parameter binding of an identifier is not available.
pub fn validate_tenant(tenant: &str) -> Result<(), StoreError> {
    if tenant.is_empty() || tenant.len() > 128 {
        return Err(StoreError::Validation("tenant length".into()));
    }
    if !tenant
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(StoreError::Validation("tenant characters".into()));
    }
    Ok(())
}

/// Map a SurrealDB error to a `StoreError`. A timeout maps to `Timeout`; everything else from the
/// engine is treated as the store being unavailable (the migration left the namespace at its prior
/// version; X7 error table).
pub fn map_db_err(e: surrealdb::Error) -> StoreError {
    let msg = e.to_string();
    if msg.to_lowercase().contains("timeout") {
        StoreError::Timeout
    } else {
        StoreError::Unavailable(msg)
    }
}
