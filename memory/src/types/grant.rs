//! Consent grants (§2C, Consent Grants component).
//!
//! A grant is the fourth arm of the fact read filter: a directional, per-fact,
//! expiring permission issued by a fact's owner to one named principal. It is a
//! first-class record rather than a `Fact` with a special class because the read
//! filter joins against it, and a permission buried in opaque `content` JSON
//! cannot be indexed or appear in a `WHERE` fragment (SA-GRANT-01).
//!
//! Used by: Consent Grants (issues, revokes, lists), Memory Store (the read-filter
//! sub-select), HTTP API Edge (the grant routes).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::scope::ScopeRef;

/// The single operation a grant may confer. Read is the only value this version
/// accepts: a grant widens a read and never a write (SA-GRANT-05).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum GrantOperation {
    Read,
}

/// A directional permission from one fact's owner to one named principal. Never
/// inferred from a relationship, a team, a role, or another grant.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Grant {
    /// "grant:<uuidv7>" (SA-ID-01).
    pub id: String,
    /// The one fact this grant permits reading. Never a set, pattern or wildcard
    /// (SA-GRANT-03).
    pub fact: String,
    /// The scope that issued it. Equal to the fact's `owner` at issue time, read
    /// from the stored fact and never from request-body input (SA-GRANT-09).
    pub grantor: ScopeRef,
    /// The user id permitted to read, matched against `ScopeContext.user`. Never a
    /// team, a tenant or a wildcard (SA-GRANT-03).
    pub grantee: String,
    /// Why the grantee may read it. Recorded for audit and shown to the grantor on
    /// review; 1..=512 chars, never interpreted by recall.
    pub purpose: String,
    pub operation: GrantOperation,
    /// A grant is inert before this instant.
    pub valid_from: DateTime<Utc>,
    /// Required — a grant with no expiry is not expressible (SA-GRANT-04).
    pub valid_to: DateTime<Utc>,
    /// Set on revocation. The row is retained, never deleted, so the audit trail
    /// can answer what was readable when (SA-GRANT-07).
    pub revoked_at: Option<DateTime<Utc>>,
    /// What the grantor relied on when consenting, recorded verbatim.
    pub evidence: String,
    pub created_at: DateTime<Utc>,
}

impl Grant {
    /// Whether this grant admits a read at `now`. Mirrors the SQL sub-select in
    /// `store::grant_arm_sql` exactly; the two are asserted equivalent by test.
    ///
    /// This is a convenience for in-process assertions and audit rendering. It is
    /// **not** the enforcement point — enforcement is the query (SA-GRANT-01).
    pub fn is_active_at(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.valid_from <= now && self.valid_to > now
    }
}

/// The request to issue a grant. `grantor` is absent by construction: it is read
/// from the stored fact's owner, so a caller cannot assert authority it lacks.
#[derive(Deserialize, Clone, Debug)]
pub struct IssueGrant {
    pub fact: String,
    pub grantee: String,
    pub purpose: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    pub evidence: String,
}

/// Which side of a grant the caller is asking about.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum GrantSide {
    /// Grants this caller issued.
    Issued,
    /// Grants naming this caller as grantee.
    Received,
}

#[derive(Deserialize, Clone, Debug)]
pub struct GrantQuery {
    pub side: GrantSide,
    /// When set, restrict to grants over this fact.
    pub fact: Option<String>,
    /// When true, omit revoked and expired grants.
    pub active_only: bool,
    pub limit: u32,
    pub offset: u32,
}
