//! Memory profile and physical placement — the seam that lets one data model serve two scenarios.
//!
//! `memory` serves two deployment scenarios over the **same** data model (Tenant → Team → User with a
//! per-fact visibility). They differ in the *expected visible fraction per caller*, and that one fact
//! drives every other difference:
//!
//! * **Collective memory** — many users share one memory. A caller sees their own facts plus everything
//!   shared to their team and tenant, so the visible fraction is high.
//! * **Individual memory** — every user has their own memory and shares nothing, so a caller sees only
//!   their own facts and the visible fraction is `1/N` of whatever else lives alongside.
//!
//! Because retrieval strategy, vector-index provisioning, topology and sweep strategy are all
//! consequences of that single fact, they are selected together by one named profile rather than by
//! independent switches — independent switches would admit incoherent combinations (filter-first search
//! *and* an ANN index; many users per namespace *and* post-filtered search).
//!
//! **Placement is still identity under both profiles.** Selecting `individual` already changes *how* a
//! namespace is searched (the filter-first retrieval strategy, Stage 1); it does not yet change *which*
//! namespace a tenant lives in. That is Stage 2, and it is a change to `resolve` alone rather than to
//! the 38 call sites that once selected a namespace in C1.

use std::fmt;

/// Which deployment scenario a `memory` instance serves (`MEMORY_PROFILE`).
///
/// Selecting a profile selects, as a coherent set: the stage-1 vector retrieval strategy, whether the
/// HNSW vector index is provisioned, how a logical tenant maps to a physical namespace, and how the
/// maintenance sweep enumerates work.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MemoryProfile {
    /// Many users share one memory; the visible fraction per caller is high. The shipped default, and
    /// the only profile whose behaviour is fully implemented.
    #[default]
    Collective,
    /// Every user has their own memory and shares nothing; the visible fraction per caller is `1/N`.
    ///
    /// **Live since Stage 1**, and distinct from `Collective` in one respect so far: it selects the
    /// filter-first retrieval strategy and the narrow owner read-filter (`store::vector_read_filter`).
    /// Placement, the optional vector index and multi-tenant namespaces attach here in Stage 2, which
    /// is blocked on RISK-017 — the read-filter's `tenant-shared` arm carries no tenant predicate and
    /// is safe only while one namespace holds one tenant.
    Individual,
}

impl MemoryProfile {
    /// Parse the configured value. Unknown values are rejected rather than defaulted, so a typo fails
    /// startup instead of silently selecting the wrong scenario.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "collective" => Some(Self::Collective),
            "individual" => Some(Self::Individual),
            _ => None,
        }
    }

    /// The configured spelling, for logs and diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Collective => "collective",
            Self::Individual => "individual",
        }
    }

    /// Whether a caller in this profile can expect to see a high fraction of a namespace's facts.
    ///
    /// This is the property every downstream difference derives from. It is exposed as a predicate so
    /// later stages read as *why* they branch rather than *which enum variant* they matched.
    pub fn expects_high_visible_fraction(self) -> bool {
        match self {
            Self::Collective => true,
            Self::Individual => false,
        }
    }

    /// Whether a namespace under this profile holds exactly one tenant.
    ///
    /// This is what `REMOVE NAMESPACE` rests on: dropping a namespace is equivalent to erasing one
    /// tenant only while the answer is `true`. Both profiles still say `true` because placement is
    /// still identity; the Stage 2 edit that maps many tenants onto one namespace flips the
    /// `Individual` arm here in the same change as `resolve` (ADR-018, RISK-021).
    pub fn namespaces_hold_one_tenant(self) -> bool {
        match self {
            Self::Collective | Self::Individual => true,
        }
    }
}

impl fmt::Display for MemoryProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a logical tenant's data physically lives: a SurrealDB namespace and database.
///
/// `ScopeContext.tenant` answers *who the caller belongs to*. `Placement` answers *where their data
/// is*. Those are the same string in the collective profile and need not be in the individual one;
/// keeping them distinct types is what makes a later change local.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Placement {
    pub namespace: String,
    pub database: String,
}

/// Resolve a logical tenant to its physical placement under `profile`.
///
/// **Both profiles still resolve to identity** — namespace = tenant, database = `default_database`.
/// Retrieval already differs by profile; placement does not, and keeping the two apart is what let the
/// retrieval change ship on its own. Stage 2 changes this function's `Individual` arm alone to map many
/// tenants onto one namespace.
pub fn resolve(profile: MemoryProfile, tenant: &str, default_database: &str) -> Placement {
    match profile {
        MemoryProfile::Collective | MemoryProfile::Individual => Placement {
            namespace: tenant.to_string(),
            database: default_database.to_string(),
        },
    }
}

/// Whether `tenant` has `placement`'s namespace to itself, and so whether an unbounded
/// namespace-level destructive operation can stand in for erasing that tenant (ADR-018).
///
/// Two independent checks, both of which must hold. The profile must *declare* that its namespaces
/// hold one tenant each, and the resolved placement must put the tenant on a namespace of its own
/// name. The redundancy is deliberate: a later topology change that shares namespaces trips the
/// second check even if it forgets to update the first, and what this guards is irreversible
/// destruction on the path a data subject invokes to exercise a right (RISK-021).
pub fn namespace_is_exclusive(profile: MemoryProfile, tenant: &str, placement: &Placement) -> bool {
    profile.namespaces_hold_one_tenant() && placement.namespace == tenant
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_exactly_the_two_profiles_and_rejects_anything_else() {
        assert_eq!(
            MemoryProfile::parse("collective"),
            Some(MemoryProfile::Collective)
        );
        assert_eq!(
            MemoryProfile::parse("individual"),
            Some(MemoryProfile::Individual)
        );
        // A typo must fail startup rather than silently select a scenario.
        assert_eq!(MemoryProfile::parse("Collective"), None);
        assert_eq!(MemoryProfile::parse("consumer"), None);
        assert_eq!(MemoryProfile::parse(""), None);
    }

    #[test]
    fn collective_is_the_default() {
        assert_eq!(MemoryProfile::default(), MemoryProfile::Collective);
    }

    #[test]
    fn visible_fraction_expectation_is_what_separates_the_profiles() {
        assert!(MemoryProfile::Collective.expects_high_visible_fraction());
        assert!(!MemoryProfile::Individual.expects_high_visible_fraction());
    }

    /// Exclusivity holds under both profiles today, because placement is identity under both. The
    /// Stage 2 edit flips the `Individual` arm of `resolve`, and this test with it.
    #[test]
    fn a_namespace_is_exclusive_to_its_tenant_under_both_profiles() {
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            let p = resolve(profile, "acme", "memory");
            assert!(
                namespace_is_exclusive(profile, "acme", &p),
                "{profile}: one tenant per namespace must still hold"
            );
        }
    }

    /// The structural half of the guard bites on its own, without waiting for the profile predicate
    /// to be updated. This is the Stage 2 shape — a tenant resolved onto a pooled namespace — and it
    /// is the case the redundancy exists for, since a topology change edits `resolve`.
    #[test]
    fn exclusivity_fails_when_the_mapping_moves_a_tenant_off_its_own_namespace() {
        let pooled = Placement {
            namespace: "pool_0".into(),
            database: "memory".into(),
        };
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            assert!(
                profile.namespaces_hold_one_tenant(),
                "{profile}: the declared half still says one tenant per namespace"
            );
            assert!(
                !namespace_is_exclusive(profile, "acme", &pooled),
                "{profile}: a pooled namespace is not exclusive to the tenant"
            );
        }
    }

    /// Placement is identity under BOTH profiles, so no stage so far can have moved an existing
    /// deployment's data. Stage 2 changes the `Individual` arm and this test with it.
    #[test]
    fn placement_is_identity_under_both_profiles() {
        for profile in [MemoryProfile::Collective, MemoryProfile::Individual] {
            let p = resolve(profile, "acme", "memory");
            assert_eq!(
                p,
                Placement {
                    namespace: "acme".into(),
                    database: "memory".into()
                },
                "{profile}: placement must still be identity"
            );
        }
    }
}
