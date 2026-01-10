//! The error-code registry (ErrorHandling.md *Error-code registry*), compiled in as data.
//!
//! `ordin-types` depends on no workspace crate, so it cannot read another crate's codes; each crate
//! instead exports `ALL_CODES` and its own test asks [`lookup`] and [`owns_prefix`] about every entry
//! (ErrorHandling.md *Internal Logic* 11).
//!
//! The table is generated from ErrorHandling.md with these readings of its columns:
//! - `owner_crate` is the package of the row's *Owner*, ignoring the *also* list. SPEC-GAP-9: six rows
//!   name two owners joined by `+` (`HOOK_APPROVAL_PENDING`, `KQ_CONFIG_INVALID`, `KQ_STORE_UNAVAILABLE`,
//!   `SRV_METHOD_NOT_ALLOWED`, `SRV_NO_ROUTE`, `SRV_REFUSED`) while the entry holds one crate; the first
//!   is recorded. The Screens Module owns `UI_` and is recorded as its npm package `@ordin/screens`.
//! - `http_status` is the first status the row gives. SPEC-GAP-10: thirteen rows give two statuses (one
//!   per surface, for example `403 (Caller API); 200 kind: error (Remote API)`); the first is recorded.
//! - `retryable` is true for *yes*. SPEC-GAP-11: twenty-four rows read *yes / no (per row)* or
//!   *(per cause)*, which one flag cannot carry; they are recorded as false, and the raising module's
//!   `CodedError::retryable` decides per occurrence.
//! - `sentence_id` is the code in kebab-case (`appr-card-closed`).

mod table;

/// One row of the registry, compiled in from the table below.
#[derive(Debug)]
pub struct RegistryEntry {
    pub code: &'static str,
    pub owner_crate: &'static str, // e.g. "ordin-approval"
    pub http_status: Option<u16>,  // Caller API or Remote API status; None elsewhere
    pub retryable: bool,
    pub sentence_id: &'static str, // Fluent message id, the code in kebab-case ("appr-card-closed")
}

/// Look up a code; None when it is not registered.
pub fn lookup(code: &str) -> Option<&'static RegistryEntry> {
    table::REGISTRY
        .binary_search_by(|entry| entry.code.as_bytes().cmp(code.as_bytes()))
        .ok()
        .and_then(|at| table::REGISTRY.get(at))
}

/// True when `prefix` (without the underscore) is owned by `crate_name`.
pub fn owns_prefix(crate_name: &str, prefix: &str) -> bool {
    table::PREFIX_OWNERS
        .binary_search_by(|(owned, _)| owned.as_bytes().cmp(prefix.as_bytes()))
        .ok()
        .and_then(|at| table::PREFIX_OWNERS.get(at))
        .is_some_and(|(_, owners)| owners.contains(&crate_name))
}
