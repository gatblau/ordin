//! Scope & auth context (§2C.3).
//!
//! Used by: C3 (produces), C1/C4/C6/C7/C8 (consume — every query is scoped).

use serde::{Deserialize, Serialize};

use crate::types::domain::Visibility;

/// The owning scope stored on every record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ScopeRef {
    /// Tenant id -> SurrealDB namespace (ADR-011).
    pub tenant: String,
    /// Team id, null for user-only facts.
    pub team: Option<String>,
    /// User id, bound to the OIDC subject claim.
    pub user: String,
}

/// The authenticated request context derived by C3 from the validated token.
/// Never constructed from request-body input.
#[derive(Clone)]
pub struct ScopeContext {
    pub tenant: String,
    /// Membership claim — teams the user belongs to.
    pub teams: Vec<String>,
    /// = token subject claim.
    pub user: String,
    /// For the audit trail (never the token itself).
    pub token_jti: String,
    /// read / write / forget, from token scopes.
    pub allowed_ops: OpSet,
    /// Calling service / client identity, from the token's `azp` (or the configured claim).
    /// `None` when the token carries none — never substituted with `user`.
    // spec: components/auth-scope#SA-AUTHZ-08
    pub client: Option<String>,
    /// Which fact classes this token may assert. `None` = the claim was absent = every class;
    /// `Some(empty)` = no class may be asserted, which is a different statement.
    // spec: components/auth-scope#SA-AUTHZ-02
    pub fact_authority: Option<Vec<FactAuthority>>,
    pub correlation_id: String,
}

/// One entry of the `memory_fact_authority` claim: a class this token may assert, and optionally the
/// single source it may attribute that class to.
///
/// Both fields are **opaque strings** belonging to the caller's vocabulary. `memory` compares them
/// and never interprets either — it holds no notion of what any class means, and acquiring one would
/// breach the LLM-free constraint the read and write paths are both built on.
// spec: phase-1-analysis#SA-AUTHZ-01
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FactAuthority {
    /// Non-empty.
    pub class: String,
    /// Non-empty when present. Compared against `RememberRequest.source.origin_ref`.
    pub source: Option<String>,
}

impl ScopeContext {
    /// Whether this context may assert `fact_class`, attributing it to `origin_ref`.
    ///
    /// Returns [`ClassAuthorityDecision`] rather than a bool so the caller can tell the three
    /// outcomes apart: they carry different metrics and, for `AdmittedNoClaim`, the counter that
    /// makes an absent control visible instead of silent.
    // spec: phase-1-analysis#SA-AUTHZ-01
    pub fn may_assert_class(
        &self,
        fact_class: Option<&str>,
        origin_ref: Option<&str>,
    ) -> ClassAuthorityDecision {
        // An unclassed write asserts no class, so there is nothing to hold authority over. This is
        // the branch every caller predating the control takes, and it is why the existing write
        // suite passes unchanged.
        let Some(class) = fact_class else {
            return ClassAuthorityDecision::Admitted;
        };
        // An absent claim grants every class (the compatibility default). Reported, not silent.
        let Some(entries) = self.fact_authority.as_ref() else {
            return ClassAuthorityDecision::AdmittedNoClaim;
        };
        // A class the claim never names is refused before any source comparison, so the refusal
        // reason distinguishes "you may not assert this at all" from "not for that source".
        let mut class_named = false;
        for e in entries {
            if e.class != class {
                continue;
            }
            class_named = true;
            match e.source.as_deref() {
                // Bound to no source: any attribution, including none, is admitted.
                None => return ClassAuthorityDecision::Admitted,
                // Bound to a source: exact match only. A request carrying no source is refused,
                // because "attributed to nothing" is not "attributed to S".
                Some(bound) if Some(bound) == origin_ref => {
                    return ClassAuthorityDecision::Admitted
                }
                Some(_) => {}
            }
        }
        if class_named {
            ClassAuthorityDecision::RefusedSource
        } else {
            ClassAuthorityDecision::RefusedClass
        }
    }
}

/// The outcome of the fact-class authority check.
// spec: phase-1-analysis#SA-AUTHZ-01
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassAuthorityDecision {
    /// Admitted: either no class was asserted, or the claim covers it.
    Admitted,
    /// Admitted only because the token carried no authority claim. Counted, so a deployment can
    /// tell that the control is not in force rather than discovering it from a write it expected
    /// to be refused.
    // spec: phase-1-analysis#SA-AUTHZ-02
    AdmittedNoClaim,
    /// The claim names no entry for this class.
    RefusedClass,
    /// The claim names this class, but bound to a different source.
    // spec: components/service-layer#SA-AUTHZ-05
    RefusedSource,
}

impl ClassAuthorityDecision {
    /// `true` when the write may proceed.
    pub fn is_admitted(self) -> bool {
        matches!(self, Self::Admitted | Self::AdmittedNoClaim)
    }

    /// The `reason` label for `memory_write_authority_refusals_total`, or `None` when admitted.
    /// The class label itself is never a metric label: the vocabulary is the caller's and
    /// unbounded, so it would make cardinality a function of somebody else's domain model.
    pub fn refusal_reason(self) -> Option<&'static str> {
        match self {
            Self::RefusedClass => Some("class"),
            Self::RefusedSource => Some("source"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct OpSet {
    pub read: bool,
    pub write: bool,
    pub forget: bool,
}

/// Read filter rule (binding for every store query, §2C.3).
///
/// A caller may read a Fact/Entity/Relationship iff
/// `record.owner.tenant == ctx.tenant` **and** (
///   `record.owner.user == ctx.user`
///   **or** (`record.visibility == TeamShared` and `record.owner.team ∈ ctx.teams`)
///   **or** `record.visibility == TenantShared`
/// ).
///
/// Cross-tenant access is structurally impossible — a different tenant is a different namespace —
/// but this helper enforces the tenant match defensively as well.
pub fn can_read(ctx: &ScopeContext, owner: &ScopeRef, visibility: Visibility) -> bool {
    if owner.tenant != ctx.tenant {
        return false;
    }
    if owner.user == ctx.user {
        return true;
    }
    match visibility {
        Visibility::TeamShared => owner
            .team
            .as_ref()
            .is_some_and(|t| ctx.teams.iter().any(|ct| ct == t)),
        Visibility::TenantShared => true,
        Visibility::UserPrivate => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(tenant: &str, user: &str, teams: &[&str]) -> ScopeContext {
        ScopeContext {
            tenant: tenant.to_string(),
            teams: teams.iter().map(|t| t.to_string()).collect(),
            user: user.to_string(),
            token_jti: "jti-1".to_string(),
            allowed_ops: OpSet {
                read: true,
                write: false,
                forget: false,
            },
            client: None,
            fact_authority: None,
            correlation_id: "c-1".to_string(),
        }
    }

    fn owner(tenant: &str, team: Option<&str>, user: &str) -> ScopeRef {
        ScopeRef {
            tenant: tenant.to_string(),
            team: team.map(|s| s.to_string()),
            user: user.to_string(),
        }
    }

    #[test]
    fn admits_own_user_private_record() {
        let c = ctx("acme", "alice", &[]);
        let o = owner("acme", None, "alice");
        assert!(can_read(&c, &o, Visibility::UserPrivate));
    }

    #[test]
    fn denies_other_users_private_record() {
        let c = ctx("acme", "alice", &["eng"]);
        let o = owner("acme", Some("eng"), "bob");
        assert!(!can_read(&c, &o, Visibility::UserPrivate));
    }

    #[test]
    fn admits_team_shared_when_member() {
        let c = ctx("acme", "alice", &["eng", "ops"]);
        let o = owner("acme", Some("eng"), "bob");
        assert!(can_read(&c, &o, Visibility::TeamShared));
    }

    #[test]
    fn denies_team_shared_when_not_member() {
        let c = ctx("acme", "alice", &["ops"]);
        let o = owner("acme", Some("eng"), "bob");
        assert!(!can_read(&c, &o, Visibility::TeamShared));
    }

    #[test]
    fn denies_team_shared_with_no_team_on_record() {
        let c = ctx("acme", "alice", &["eng"]);
        let o = owner("acme", None, "bob");
        assert!(!can_read(&c, &o, Visibility::TeamShared));
    }

    #[test]
    fn admits_tenant_shared_to_any_same_tenant_user() {
        let c = ctx("acme", "alice", &[]);
        let o = owner("acme", Some("eng"), "bob");
        assert!(can_read(&c, &o, Visibility::TenantShared));
    }

    #[test]
    fn denies_cross_tenant_even_when_tenant_shared() {
        let c = ctx("acme", "alice", &["eng"]);
        let o = owner("globex", Some("eng"), "alice");
        assert!(!can_read(&c, &o, Visibility::TenantShared));
    }

    // --- Fact-class write authority (SA-AUTHZ-01, SA-AUTHZ-02, SA-AUTHZ-05) --------------------

    fn authority(entries: &[(&str, Option<&str>)]) -> Vec<FactAuthority> {
        entries
            .iter()
            .map(|(c, src)| FactAuthority {
                class: (*c).to_string(),
                source: src.map(str::to_string),
            })
            .collect()
    }

    fn ctx_with(claim: Option<Vec<FactAuthority>>) -> ScopeContext {
        let mut c = ctx("acme", "alice", &[]);
        c.fact_authority = claim;
        c
    }

    /// One row of the class-authority table: the claim on the context, the class the write
    /// asserts, the source it attributes the fact to, and the decision expected of the pair.
    type ClassAuthorityCase<'a> = (
        Option<Vec<FactAuthority>>,
        Option<&'a str>,
        Option<&'a str>,
        ClassAuthorityDecision,
    );

    #[test]
    fn class_authority_table() {
        use ClassAuthorityDecision::*;
        let cases: &[ClassAuthorityCase] = &[
            // An unclassed write asserts no class, so nothing is judged — this is the branch every
            // caller predating the control takes, and the reason the existing suite is unchanged.
            (Some(authority(&[("a", None)])), None, None, Admitted),
            (None, None, None, Admitted),
            // An absent claim grants every class, and says so distinctly so it can be counted.
            (None, Some("anything"), None, AdmittedNoClaim),
            // A named class, unbound to a source, admits any attribution including none.
            (Some(authority(&[("a", None)])), Some("a"), None, Admitted),
            (Some(authority(&[("a", None)])), Some("a"), Some("s"), Admitted),
            // A class the claim never names.
            (Some(authority(&[("a", None)])), Some("b"), None, RefusedClass),
            // An empty array is authority over nothing — distinct from an absent claim.
            (Some(vec![]), Some("a"), None, RefusedClass),
            // Source binding: exact match admits, anything else refuses.
            (Some(authority(&[("a", Some("s"))])), Some("a"), Some("s"), Admitted),
            (Some(authority(&[("a", Some("s"))])), Some("a"), Some("t"), RefusedSource),
            // "Attributed to nothing" is not "attributed to s".
            (Some(authority(&[("a", Some("s"))])), Some("a"), None, RefusedSource),
            // Several entries for one class: any one of them may admit.
            (
                Some(authority(&[("a", Some("s")), ("a", Some("t"))])),
                Some("a"),
                Some("t"),
                Admitted,
            ),
            // Comparison is exact and case-sensitive over strings memory does not interpret.
            (Some(authority(&[("A", None)])), Some("a"), None, RefusedClass),
        ];
        for (i, (claim, class, origin, expected)) in cases.iter().enumerate() {
            let c = ctx_with(claim.clone());
            let got = c.may_assert_class(*class, *origin);
            assert_eq!(
                got, *expected,
                "case {i}: class={class:?} origin={origin:?}"
            );
        }
    }

    #[test]
    fn refusal_reason_distinguishes_class_from_source() {
        assert_eq!(
            ClassAuthorityDecision::RefusedClass.refusal_reason(),
            Some("class")
        );
        assert_eq!(
            ClassAuthorityDecision::RefusedSource.refusal_reason(),
            Some("source")
        );
        assert_eq!(ClassAuthorityDecision::Admitted.refusal_reason(), None);
        assert_eq!(
            ClassAuthorityDecision::AdmittedNoClaim.refusal_reason(),
            None
        );
    }

    #[test]
    fn admitted_variants_are_the_two_that_proceed() {
        assert!(ClassAuthorityDecision::Admitted.is_admitted());
        assert!(ClassAuthorityDecision::AdmittedNoClaim.is_admitted());
        assert!(!ClassAuthorityDecision::RefusedClass.is_admitted());
        assert!(!ClassAuthorityDecision::RefusedSource.is_admitted());
    }
}
