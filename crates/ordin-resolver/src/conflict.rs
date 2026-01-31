//! The conflict shapes returned by `RES_CONFLICT` and by `conflicts` (ResolverAndCompiler.md *Public
//! Interface*).

use ordin_agent_model::PartRef;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Conflict {
    /// `<Kind>/<name>`: the slot two answers compete for.
    pub slot: String,
    pub candidates: Vec<ConflictCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConflictCandidate {
    pub part: PartRef,
    pub requested_by: Vec<PartRef>,
}
