//! C1 — Memory Store. The storage implementations behind the storage port
//! (`crate::types::ports::StoragePort`, ADR-101 item 1).
//!
//! Every component above this module — the Service, the write pipeline, the maintenance worker, the
//! lease reaper and the retrieval engine — holds an `Arc<dyn StoragePort>` and names no storage
//! engine. The SurrealDB implementation lives in [`surreal`] (feature `surreal`); no module outside it
//! names a SurrealDB type. The SQLite and PostgreSQL implementation lives in [`sql`] (feature `sql`).

pub(crate) mod fusion;
pub(crate) mod graph;
#[cfg(feature = "sql")]
pub mod sql;
#[cfg(feature = "surreal")]
pub mod surreal;

use std::collections::{BTreeMap, HashSet};

use serde_json::Value as Json;
use sha2::{Digest, Sha256};

use crate::types::ports::StoreError;
use crate::types::scope::{OpSet, ScopeContext, ScopeRef};

/// The scope an erasure of a team or a team's folder sweeps under: not bound to one owner, and
/// within the team (another team's summary is outside the erasure).
pub(crate) fn erasure_scope(tenant: &str, team: &str) -> ScopeContext {
    ScopeContext {
        tenant: tenant.to_string(),
        teams: vec![team.to_string()],
        user: String::new(),
        token_jti: "maintenance".to_string(),
        allowed_ops: OpSet {
            read: true,
            write: true,
            forget: true,
        },
        client: None,
        fact_authority: None,
        correlation_id: String::new(),
    }
}

/// Whether `ctx` reads `owner_user`'s record as its owner: the user, or the C7 maintenance scope, whose
/// duties reach the whole tenant.
pub(crate) fn owner_view(ctx: &ScopeContext, owner_user: &str) -> bool {
    ctx.user == owner_user || (ctx.token_jti == "maintenance" && ctx.user.is_empty())
}

/// C1's ownership check (SA-AUTHZ-07): a record the read filter admits may be changed or removed by its
/// owner alone. Reading is not owning — a teammate's or a grantee's call stops here.
// spec: phase-1-analysis#SA-AUTHZ-07
pub(crate) fn owner_gate(ctx: &ScopeContext, owner_user: &str) -> Result<(), StoreError> {
    if owner_view(ctx, owner_user) {
        Ok(())
    } else {
        Err(StoreError::ScopeForbidden)
    }
}

/// The key a held write is stored under: the caller's key within its own tenant, team and user, as the
/// job queue dedupes it, so no other caller's key lands on the row.
// `mem_quarantine_idem` and `quarantine_idem_idx` are unique on `idempotency_key` alone, so the column
// holds this digest, not the caller's key.
pub(crate) fn scoped_quarantine_key(owner: &ScopeRef, key: &str) -> String {
    let seed = serde_json::json!([owner.tenant, owner.team, owner.user, key]).to_string();
    Sha256::digest(seed.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The normalised names a `remember` payload, or a held write's columns, carry: `content.subject` and
/// `content.object`, the stated entities with their aliases, and both ends of each stated link.
pub(crate) fn names_in_write(content: &Json, entities: &Json, links: &Json) -> HashSet<String> {
    let mut out = HashSet::new();
    for key in ["subject", "object"] {
        if let Some(s) = content.get(key).and_then(Json::as_str) {
            out.insert(crate::text::normalise_name(s));
        }
    }
    for e in entities.as_array().into_iter().flatten() {
        if let Some(n) = e.get("name").and_then(Json::as_str) {
            out.insert(crate::text::normalise_name(n));
        }
        for a in e
            .get("aliases")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(a) = a.as_str() {
                out.insert(crate::text::normalise_name(a));
            }
        }
    }
    for l in links.as_array().into_iter().flatten() {
        for end in ["from", "to"] {
            if let Some(n) = l.get(end).and_then(Json::as_str) {
                out.insert(crate::text::normalise_name(n));
            }
        }
    }
    out.remove("");
    out
}

/// Whether a job payload, queued or failed, names one of `names`.
pub(crate) fn payload_names(payload: &Json, names: &HashSet<String>) -> bool {
    let none = Json::Null;
    let content = payload.get("content").unwrap_or(&none);
    let entities = payload.get("entities").unwrap_or(&none);
    let links = payload.get("links").unwrap_or(&none);
    !names_in_write(content, entities, links).is_disjoint(names)
}

/// Render a JSON content object to a flat keyword string for the BM25 `content_text` index: every
/// scalar leaf concatenated with spaces. The original structured `content` object is stored intact.
pub fn content_to_text(content: &Json) -> String {
    let mut out = String::new();
    fn walk(v: &Json, out: &mut String) {
        match v {
            Json::String(x) => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(x);
            }
            Json::Number(x) => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&x.to_string());
            }
            Json::Bool(x) => {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(if *x { "true" } else { "false" });
            }
            Json::Array(a) => a.iter().for_each(|e| walk(e, out)),
            Json::Object(m) => m.values().for_each(|e| walk(e, out)),
            Json::Null => {}
        }
    }
    walk(content, &mut out);
    out
}

/// The digest lines of an `erase_collection` proof: `tenant=`, `namespace=`, `team=`, `collection=`,
/// then one `<table>=<count>` line per table in key order (SA-ERASE-01's form with two lines added).
pub(crate) fn collection_erasure_digest_input(
    tenant: &str,
    namespace: &str,
    team: &str,
    collection: &str,
    removed: &BTreeMap<String, u64>,
) -> Vec<String> {
    let mut lines = Vec::with_capacity(removed.len() + 4);
    lines.push(format!("tenant={tenant}"));
    lines.push(format!("namespace={namespace}"));
    lines.push(format!("team={team}"));
    lines.push(format!("collection={collection}"));
    for (table, count) in removed {
        lines.push(format!("{table}={count}"));
    }
    lines
}
