//! The Organisation Profile (OrganisationProfile.md): reads the company's signed organisation profile,
//! verifies its Ed25519 detached signature, checks every value against the bounds of 2D-2 and serves
//! the result as one typed, immutable snapshot. It also owns each remote agent's status, kept in step
//! with the profile's remote agent catalogue.
//!
//! Two cross-cutting parts land here (Generation Playbook §2): Configuration's profile loader for the
//! desktop and the personal server (`OrganisationProfileService`, `ProfileSnapshot`,
//! `InstallerSettings`), and InputValidation of the profile at load (`PROF_` codes). The crate has no
//! Journal edge (SA-RECON-01): the program hands it the journal's SQLite pool and registers
//! [`MODULE_NAME`] and [`SQLITE_MIGRATIONS`] with the Journal's migration registry.
//!
//! The crate reads no environment variable; the program passes the application data root and the
//! HTTP client.
#![forbid(unsafe_code)]

mod document;
mod error;
mod fetch;
mod installer;
mod jws;
mod service;
mod snapshot;
mod store;
mod validate;

#[cfg(test)]
mod tests;

pub use document::*;
pub use error::{ProfileError, ALL_CODES};
pub use installer::InstallerSettings;
pub use ordin_types::domain::agents::{RemoteAgent, RemoteAgentStatus, RemoteRoute};
pub use ordin_types::time::Timestamp;
pub use ordin_types::{OrganisationProfileId, RemoteAgentId};
pub use service::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
pub use snapshot::{
    ProfileFailure, ProfileSnapshot, ProfileState, ProfileStatus, RefreshOutcome, SnapshotSource,
};

/// The `component` field of every log line (Logging.md; OrganisationProfile.md *Observability*).
const COMPONENT: &str = "organisation_profile";

/// The one profile format version this build reads (*Configuration*).
const FORMAT_VERSION: u32 = 1;

/// Unit tests reach the `wiremock` stand-in over plain HTTP on loopback; every build that ships accepts
/// `https` addresses only.
const ALLOW_LOOPBACK_HTTP: bool = cfg!(test);

/// The module name under which the program registers [`SQLITE_MIGRATIONS`] (DatabaseMigrations.md
/// *Modules that register migrations*).
// spec: ordin-desktop/components/OrganisationProfile#SA-RECON-01
pub const MODULE_NAME: &str = "organisation_profile";

/// `(version, description, SQL)`: the program builds
/// `ModuleMigrations { module: MODULE_NAME, sqlite: SQLITE_MIGRATIONS, postgres: &[] }`. No PostgreSQL
/// variant: the agent profile reads no organisation profile.
pub const SQLITE_MIGRATIONS: &[(i64, &str, &str)] = &[(
    1,
    "create organisation_profiles and remote_agents",
    MIGRATION_1,
)];

/// *Data Model*, step 1.
const MIGRATION_1: &str = "\
CREATE TABLE organisation_profiles (
    id               TEXT PRIMARY KEY CHECK (length(id) = 36),
    profile_version  TEXT NOT NULL CHECK (length(profile_version) BETWEEN 1 AND 64),
    signature        TEXT NOT NULL CHECK (length(signature) BETWEEN 1 AND 8192),
    document_digest  TEXT NOT NULL UNIQUE CHECK (length(document_digest) = 64),
    cached_path      TEXT NOT NULL CHECK (length(cached_path) BETWEEN 1 AND 4096),
    state            TEXT NOT NULL CHECK (state IN ('fetched','verified','active','replaced','failed_verification')),
    refusal_reason   TEXT CHECK (refusal_reason IS NULL OR length(refusal_reason) <= 500),
    fetched_at       TEXT NOT NULL,
    verified_at      TEXT,
    activated_at     TEXT,
    CHECK ((state = 'failed_verification') = (refusal_reason IS NOT NULL)),
    CHECK (state NOT IN ('active','replaced') OR (verified_at IS NOT NULL AND activated_at IS NOT NULL))
);
CREATE UNIQUE INDEX organisation_profiles_one_active ON organisation_profiles (state) WHERE state = 'active';
CREATE INDEX organisation_profiles_fetched ON organisation_profiles (fetched_at DESC);

CREATE TABLE remote_agents (
    id                     TEXT PRIMARY KEY CHECK (length(id) = 36),
    display_name           TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    route                  TEXT NOT NULL CHECK (route IN ('http','slack')),
    deployment_address     TEXT CHECK (deployment_address IS NULL OR length(deployment_address) <= 2048),
    slack_account          TEXT CHECK (slack_account IS NULL OR length(slack_account) <= 80),
    slack_channel          TEXT CHECK (slack_channel IS NULL OR length(slack_channel) <= 80),
    identity_provider_ref  TEXT CHECK (identity_provider_ref IS NULL OR length(identity_provider_ref) <= 120),
    status                 TEXT NOT NULL CHECK (status IN ('listed','enabled','verified','withdrawn')),
    listed_at              TEXT NOT NULL,
    CHECK (route <> 'http'  OR (deployment_address IS NOT NULL AND identity_provider_ref IS NOT NULL
                                AND slack_account IS NULL AND slack_channel IS NULL)),
    CHECK (route <> 'slack' OR (slack_account IS NOT NULL AND slack_channel IS NOT NULL
                                AND deployment_address IS NULL))
);
CREATE UNIQUE INDEX remote_agents_name_live ON remote_agents (display_name) WHERE status <> 'withdrawn';
";
