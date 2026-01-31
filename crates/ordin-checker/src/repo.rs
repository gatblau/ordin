//! Section D as a whole (steps 9–13) and the lifecycle checks that read other parts (step 8): the
//! checks of a repository with a change applied, all or nothing (ADR-091 item 10).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ordin_agent_model::{
    relation_kind, Manifest, PartKind, PartSource, Resolution, SourceScope, SourcedPart,
};
use ordin_resolver::{Conflict, ResolveError, ResolverAndCompiler};

use crate::body::strings;
use crate::context::{resolve_request, scope_of, Gathered, Item};
use crate::cross::{cross_checks, finding_key, ingest_rules, offered_pack_ids, overlay_offers};
use crate::error::{from_source, resolver_failure, CheckError};
use crate::finding::{finding, msg, part_failure};
use crate::log;
use crate::model::{Finding, Fix, RepositoryCheck, Severity};
use crate::stage_one::{self, PartView};

/// Where a reference may be looked up beyond the parts in hand.
pub(crate) struct Lookup<'a> {
    pub source: &'a dyn PartSource,
    pub scope: &'a SourceScope,
}

/// Every reference of one part as (kind, identifier): the relation keys, the packs and overlays an
/// agent offers. The offers are kept only when `chosen` holds them; `None` keeps every offer
/// (CHK-INS-1).
// spec: ordin-desktop/components/Checker#CHK-INS-1
fn references_of_chosen(item: &Item, chosen: Option<&BTreeSet<String>>) -> Vec<(PartKind, String)> {
    let kept = |id: &String| chosen.is_none_or(|c| c.contains(id));
    let mut out: Vec<(PartKind, String)> = item
        .manifest
        .references()
        .into_iter()
        .filter_map(|r| relation_kind(&r.relation).map(|kind| (kind, r.id)))
        .collect();
    if item.kind == PartKind::WorkspaceProfile {
        out.extend(
            offered_pack_ids(item)
                .into_iter()
                .filter(|id| kept(id))
                .map(|id| (PartKind::PracticePack, id)),
        );
        for (axis, available, default) in overlay_offers(item) {
            out.extend(
                available
                    .into_iter()
                    .chain(default)
                    .filter(|id| kept(id))
                    .map(|id| (axis, id)),
            );
        }
    }
    let mut seen = BTreeSet::new();
    out.retain(|r| seen.insert(r.clone()));
    out
}

/// Where a part was found.
pub(crate) enum Found {
    /// Among the parts in hand (the union or the lock's parts).
    InHand,
    Source(Box<SourcedPart>),
    Missing,
    /// Carried by this many repositories at one step of the order (ADR-090 §2).
    Ambiguous(usize),
}

/// A part of `kind` and `id`: from the parts in hand, else through the source.
pub(crate) async fn resolve_in(
    union: &[Item],
    lookup: Option<&Lookup<'_>>,
    kind: PartKind,
    id: &str,
) -> Result<Found, CheckError> {
    if union.iter().any(|i| i.kind == kind && i.id() == id) {
        return Ok(Found::InHand);
    }
    let Some(lookup) = lookup else {
        return Ok(Found::Missing);
    };
    match lookup
        .source
        .resolve(lookup.scope, kind, id, None)
        .await
        .map_err(from_source)?
    {
        Resolution::Found(sourced) => Ok(Found::Source(Box::new(sourced))),
        Resolution::NotFound => Ok(Found::Missing),
        Resolution::Ambiguous { repositories } => Ok(Found::Ambiguous(repositories.len())),
    }
}

/// The manifest of a part of `kind` and `id`, from the parts in hand or the source; None when it
/// cannot be found or is ambiguous.
async fn manifest_in(
    union: &[Item],
    lookup: Option<&Lookup<'_>>,
    kind: PartKind,
    id: &str,
) -> Result<Option<Manifest>, CheckError> {
    if let Some(item) = union.iter().find(|i| i.kind == kind && i.id() == id) {
        return Ok(Some(item.manifest.clone()));
    }
    match resolve_in(union, lookup, kind, id).await? {
        Found::Source(sourced) => Ok(Some(sourced.manifest.clone())),
        _ => Ok(None),
    }
}

/// Step 11: every reference of `items` resolves within `union`, then through the source.
pub(crate) async fn references(
    items: &[&Item],
    union: &[Item],
    lookup: Option<&Lookup<'_>>,
) -> Result<Vec<Finding>, CheckError> {
    references_chosen(items, union, lookup, None).await
}

/// Step 11 for an installed agent (step 29, CHK-INS-1): a profile's offers count only when the lock
/// chose them (`chosen`); `None` is step 11 as written.
// spec: ordin-desktop/components/Checker#CHK-INS-1
pub(crate) async fn references_chosen(
    items: &[&Item],
    union: &[Item],
    lookup: Option<&Lookup<'_>>,
    chosen: Option<&BTreeSet<String>>,
) -> Result<Vec<Finding>, CheckError> {
    let mut out = Vec::new();
    for item in items {
        for (kind, id) in references_of_chosen(item, chosen) {
            if union.iter().any(|i| i.id() == id) {
                continue;
            }
            match resolve_in(union, lookup, kind, &id).await? {
                Found::InHand | Found::Source(_) => {}
                Found::Missing => out.push(part_failure(
                    "reference_unresolved",
                    item.id(),
                    item.name(),
                    vec![Fix::AskOrdin],
                    format!("{} refers to {id}, which cannot be found", item.id()),
                )),
                Found::Ambiguous(repositories) => out.push(part_failure(
                    "reference_ambiguous",
                    item.id(),
                    item.name(),
                    vec![Fix::AskOrdin],
                    format!(
                        "{} refers to {id}, which {} repositories carry",
                        item.id(),
                        repositories
                    ),
                )),
            }
        }
    }
    Ok(out)
}

fn lifecycle_failure(item: &Item, code: &str, field: &str, detail: String) -> Finding {
    finding(
        code,
        Severity::Failure,
        Some(item.id()),
        msg::LIFECYCLE_INVALID,
        &[("part", item.name())],
        vec![Fix::ChangeItYourself {
            field: field.to_owned(),
        }],
        detail,
    )
}

/// Step 8, the checks that read other parts: each stage's agent and result contract resolve, each
/// handover place is reachable, and each stage hands on the contract the next stage's agent consumes.
// `reachable_places` is keyed by the stage's `handoverPlace.target` (its `type` when the
// target is empty); a place the map does not name is not reported. The contract an agent consumes is
// the `consumes` value of the HandoffOverlay it offers by default (else of each overlay it offers).
// spec: ordin-desktop/components/Checker#ADR-095
pub(crate) async fn lifecycle_references(
    item: &Item,
    union: &[Item],
    lookup: Option<&Lookup<'_>>,
    reachable_places: &BTreeMap<String, bool>,
) -> Result<Vec<Finding>, CheckError> {
    let mut out = Vec::new();
    if !item.manifest.body.contains_key("stages") {
        return Ok(out);
    }
    let Ok(Some(journey)) = item.manifest.journey() else {
        return Ok(out);
    };
    let stages = &journey.stages;
    for (i, stage) in stages.iter().enumerate() {
        let pins = [
            (PartKind::WorkspaceProfile, &stage.agent),
            (PartKind::OutputContract, &stage.produces),
        ];
        for (kind, id) in pins {
            let Some(id) = id else { continue };
            let found = resolve_in(union, lookup, kind, id).await?;
            if !matches!(found, Found::InHand | Found::Source(_)) {
                out.push(lifecycle_failure(
                    item,
                    "lifecycle_reference_unresolved",
                    "stages",
                    format!("stage {:?} names {id}, which cannot be found", stage.id),
                ));
            }
        }
        let place = if stage.handover_place_target.is_empty() {
            &stage.handover_place_type
        } else {
            &stage.handover_place_target
        };
        if reachable_places.get(place) == Some(&false) {
            out.push(lifecycle_failure(
                item,
                "lifecycle_place_unreachable",
                "handoverPlace",
                format!(
                    "stage {:?} hands over to a place that cannot be reached",
                    stage.id
                ),
            ));
        }
        for next_id in stage_one::successors(stages, i) {
            let Some(next) = stages.iter().find(|s| s.id == next_id) else {
                continue;
            };
            let Some(agent) = &next.agent else { continue };
            let Some(agent_manifest) =
                manifest_in(union, lookup, PartKind::WorkspaceProfile, agent).await?
            else {
                continue;
            };
            let Ok(Some(offer)) = agent_manifest.overlay_offer(PartKind::HandoffOverlay) else {
                continue;
            };
            let overlays: Vec<String> = match offer.default {
                Some(default) => vec![default],
                None => offer.available,
            };
            let mut consumed: Vec<String> = Vec::new();
            for overlay in overlays {
                if let Some(manifest) =
                    manifest_in(union, lookup, PartKind::HandoffOverlay, &overlay).await?
                {
                    consumed.extend(strings(&manifest, "consumes"));
                }
            }
            let hands_on = stage
                .produces
                .as_ref()
                .is_some_and(|p| consumed.contains(p));
            if !consumed.is_empty() && !hands_on {
                out.push(finding(
                    "sync_journey_seam_unmatched",
                    Severity::Failure,
                    Some(item.id()),
                    msg::LIFECYCLE_INVALID,
                    &[("part", item.name())],
                    vec![Fix::ChangeItYourself {
                        field: "stages".to_owned(),
                    }],
                    format!(
                        "stage {:?} does not hand on the contract stage {:?} consumes",
                        stage.id, next.id
                    ),
                ));
            }
        }
    }
    Ok(out)
}

/// `parts_conflict` for one conflict (step 18): "Two parts of this agent say different things about
/// [what]: [part] and [part]".
pub(crate) fn conflict_finding(conflict: &Conflict, agent: Option<&str>) -> Finding {
    let what = conflict
        .slot
        .split_once('/')
        .map_or(conflict.slot.as_str(), |(_, name)| name);
    let name_of = |at: usize| -> String {
        conflict
            .candidates
            .get(at)
            .map(|c| {
                c.requested_by
                    .first()
                    .map_or_else(|| c.part.name.clone(), |r| r.name.clone())
            })
            .unwrap_or_default()
    };
    let (part_a, part_b) = (name_of(0), name_of(1));
    let detail = match agent {
        Some(agent) => format!(
            "agent {agent}: slot {} has {} candidates",
            conflict.slot,
            conflict.candidates.len()
        ),
        None => format!(
            "slot {} has {} candidates",
            conflict.slot,
            conflict.candidates.len()
        ),
    };
    finding(
        "parts_conflict",
        Severity::Failure,
        conflict.candidates.first().map(|c| c.part.id.as_str()),
        msg::PARTS_DISAGREE,
        &[("what", what), ("part_a", &part_a), ("part_b", &part_b)],
        vec![Fix::AskAgentMaker],
        detail,
    )
}

/// `agent_rebuild_failed` for an agent that would not rebuild with the change (step 13).
pub(crate) fn rebuild_failed(agent_id: &str, agent_name: &str, error: &ResolveError) -> Finding {
    finding(
        "agent_rebuild_failed",
        Severity::Failure,
        Some(agent_id),
        msg::AGENT_WOULD_NOT_PASS,
        &[("agent", agent_name), ("reason", error.code())],
        Vec::new(),
        format!("{agent_id} would not rebuild: {}", error.code()),
    )
}

/// The findings of a `ResolveError` that is a finding: a conflict is `parts_conflict` (step 18), any
/// other is `agent_rebuild_failed` (step 13).
pub(crate) fn resolve_error_findings(
    agent_id: &str,
    agent_name: &str,
    agent: Option<&str>,
    error: &ResolveError,
) -> Vec<Finding> {
    match error {
        ResolveError::Conflict { conflicts } if !conflicts.is_empty() => conflicts
            .iter()
            .map(|c| conflict_finding(c, agent))
            .collect(),
        other => vec![rebuild_failed(agent_id, agent_name, other)],
    }
}

/// One agent resolved with `request`, then its conflicts: the findings, or the error that stops the
/// check.
pub(crate) async fn rebuild_agent(
    resolver: &ResolverAndCompiler,
    request: &ordin_resolver::ResolveRequest,
    agent_id: &str,
    agent_name: &str,
) -> Result<Vec<Finding>, CheckError> {
    let outcome = async {
        resolver.resolve(request).await?;
        let space = resolver.pack_space(request).await?;
        resolver.conflicts(request, &space).await
    }
    .await;
    match outcome {
        Ok(conflicts) => Ok(conflicts
            .iter()
            .map(|c| conflict_finding(c, Some(agent_id)))
            .collect()),
        Err(error) => match resolver_failure(&error) {
            Some(stop) => Err(stop),
            None => Ok(resolve_error_findings(
                agent_id,
                agent_name,
                Some(agent_id),
                &error,
            )),
        },
    }
}

/// Step 13: every agent that uses a changed part is rebuilt with the change.
async fn agents_that_use(
    resolver: &ResolverAndCompiler,
    source: &Arc<dyn PartSource>,
    request: &RepositoryCheck,
    gathered: &Gathered,
) -> Result<Vec<Finding>, CheckError> {
    let mut agents: BTreeMap<String, SourcedPart> = BTreeMap::new();
    for staged in &request.changes {
        let previous = gathered
            .before
            .iter()
            .find(|i| i.kind == staged.part.kind && i.id() == staged.part.manifest.metadata.id);
        let Some(sourced) = previous.and_then(|i| i.sourced.as_ref()) else {
            continue;
        };
        for agent in source.used_by(sourced).await.map_err(from_source)? {
            agents.entry(agent.part.identifier.clone()).or_insert(agent);
        }
    }
    let mut out = Vec::new();
    for (id, agent) in agents {
        if request.deletions.contains(&id) {
            continue;
        }
        let scope = scope_of(&agent.repository, &request.scope);
        let resolve = resolve_request(
            &scope,
            &id,
            request.changes.clone(),
            Vec::new(),
            false,
            source,
        );
        let name = crate::finding::display_name(&agent.part.name, &id).to_owned();
        out.extend(rebuild_agent(resolver, &resolve, &id, &name).await?);
    }
    Ok(out)
}

/// The "Checked with the whole of [repository]" marker (11:890).
fn whole_repository(display_name: &str) -> Finding {
    finding(
        "repository_checked",
        Severity::Warning,
        None,
        msg::WHOLE_REPOSITORY,
        &[("repository", display_name)],
        Vec::new(),
        format!("checked with the whole of {display_name}"),
    )
}

/// Steps 9–13 over a gathered repository (section D), with the lifecycle checks of step 8.
// spec: ordin-desktop/components/Checker#R-80
pub(crate) async fn repository_findings(
    resolver: &ResolverAndCompiler,
    source: &Arc<dyn PartSource>,
    request: &RepositoryCheck,
    gathered: &Gathered,
    reachable_places: &BTreeMap<String, bool>,
) -> Result<Vec<Finding>, CheckError> {
    let lookup = Lookup {
        source: source.as_ref(),
        scope: &request.scope,
    };
    let is_read = request.changes.is_empty() && request.deletions.is_empty();
    let mut out = gathered.findings.clone();
    // Step 9: stage one on every part of the union.
    for item in &gathered.union {
        out.extend(stage_one::check(
            &PartView {
                kind: item.kind,
                manifest: &item.manifest,
                body: item
                    .files
                    .as_ref()
                    .and_then(|files| stage_one::body_of_files(&item.path, files)),
            },
            &gathered.vocabulary,
        ));
    }
    let checked: Vec<&Item> = gathered
        .union
        .iter()
        .filter(|i| is_read || i.changed)
        .collect();
    for item in checked.iter().filter(|i| i.kind == PartKind::Journey) {
        out.extend(
            lifecycle_references(item, &gathered.union, Some(&lookup), reachable_places).await?,
        );
    }
    // Step 10: cross-checks; for a change, only the findings it introduces.
    let mut cross = cross_checks(&gathered.union, &BTreeMap::new());
    cross.extend(ingest_rules(&gathered.union, &gathered.vocabulary));
    if !is_read {
        let mut earlier = cross_checks(&gathered.before, &BTreeMap::new());
        earlier.extend(ingest_rules(&gathered.before, &gathered.vocabulary));
        let earlier: BTreeSet<_> = earlier.iter().map(finding_key).collect();
        cross.retain(|f| {
            let pre = earlier.contains(&finding_key(f));
            if pre {
                log::preexisting_finding(&f.code, f.part_identifier.as_deref());
            }
            !pre
        });
    }
    out.extend(cross);
    // Step 11.
    out.extend(references(&checked, &gathered.union, Some(&lookup)).await?);
    // Step 13.
    if !is_read {
        out.extend(agents_that_use(resolver, source, request, gathered).await?);
    }
    out.push(whole_repository(&request.repository_display_name));
    Ok(out)
}
