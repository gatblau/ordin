//! The pack space and the conflicts across it (ResolverAndCompiler.md *Internal Logic* 23 and 24).

use std::collections::BTreeSet;

use ordin_agent_model::{kind_info, FixedPackEntry, KindFamily, PartKind, PartRef, PartSource};

use crate::closure::Closure;
use crate::conflict::Conflict;
use crate::error::{manifest_invalid, ResolveError};
use crate::held::Reader;
use crate::log;
use crate::request::{PackSpace, ResolveRequest};
use crate::resolve::load_root;
use crate::select::{read_pack_rules, Ingredients, PackRules};

/// The overlay axes an agent offers, read for `pack_space`.
const OFFERED_OVERLAY_AXES: [PartKind; 3] = [
    PartKind::SourceWorkflowOverlay,
    PartKind::StyleOverlay,
    PartKind::LanguageOverlay,
];

/// Step 23: every candidate of every required rule, every selectable pack, every fixed pack, every pack
/// and overlay **Change** offers, and every available overlay of each offered axis. Unresolvable ones are
/// skipped and logged.
pub(crate) async fn pack_space_core(
    source: &dyn PartSource,
    request: &ResolveRequest,
) -> Result<PackSpace, ResolveError> {
    let mut reader = Reader::new(
        source,
        &request.scope,
        &request.staged,
        &request.replacements,
    );
    let root = load_root(&mut reader, request).await?;
    let ingredients = Ingredients::from_list(reader.ingredients().await?);
    let rules = read_pack_rules(&root, &ingredients)?;
    let mut pack_ids: BTreeSet<String> = BTreeSet::new();
    for rule in &rules.rules {
        pack_ids.extend(rule.candidates.iter().cloned());
    }
    pack_ids.extend(rules.selectable.iter().cloned());
    for entry in &rules.fixed {
        if let FixedPackEntry::Pack(id) = entry {
            pack_ids.insert(id.clone());
        }
    }
    pack_ids.extend(request.pack_changes.iter().cloned());
    let mut packs = Vec::new();
    for id in pack_ids {
        match ingredients.packs.get(&id) {
            Some(pack) => packs.push(pack.sourced.clone()),
            None => log::pack_candidate_missing(&id),
        }
    }
    let mut overlay_ids: BTreeSet<(PartKind, String)> = BTreeSet::new();
    for axis in OFFERED_OVERLAY_AXES {
        let offer = root
            .manifest()
            .overlay_offer(axis)
            .map_err(|error| manifest_invalid(root.id(), &error))?;
        if let Some(offer) = offer {
            overlay_ids.extend(offer.available.into_iter().map(|id| (axis, id)));
        }
    }
    for id in &request.overlay_changes {
        if let Some(((kind, _), _)) = ingredients.overlays.iter().find(|((_, i), _)| i == id) {
            overlay_ids.insert((*kind, id.clone()));
        }
    }
    let mut overlays = Vec::new();
    for (kind, id) in overlay_ids {
        match ingredients.overlays.get(&(kind, id.clone())) {
            Some(overlay) => overlays.push(overlay.sourced.clone()),
            None => log::pack_candidate_missing(&id),
        }
    }
    Ok(PackSpace { packs, overlays })
}

/// True when a conflict's competing candidates never install together: all requested by overlays of one
/// exactly-one axis, or all by candidates of one rule whose `at_least` is 1.
fn never_together(conflict: &Conflict, rules: &PackRules) -> bool {
    let requesters: Vec<&PartRef> = conflict
        .candidates
        .iter()
        .flat_map(|candidate| &candidate.requested_by)
        .collect();
    let Some(first) = requesters.first() else {
        return false;
    };
    let one_axis = kind_info(first.kind).family == KindFamily::Overlay
        && kind_info(first.kind).exactly_one
        && requesters.iter().all(|r| r.kind == first.kind);
    if one_axis {
        return true;
    }
    rules.rules.iter().any(|rule| {
        rule.at_least == 1
            && requesters
                .iter()
                .all(|r| r.kind == PartKind::PracticePack && rule.candidates.contains(&r.id))
    })
}

/// Step 24: seeds are the root and every part of the space; the slot test of step 9, minus the conflicts
/// whose candidates are exclusive choices.
pub(crate) async fn conflicts_core(
    source: &dyn PartSource,
    request: &ResolveRequest,
    space: &PackSpace,
) -> Result<Vec<Conflict>, ResolveError> {
    let mut reader = Reader::new(
        source,
        &request.scope,
        &request.staged,
        &request.replacements,
    );
    let root = load_root(&mut reader, request).await?;
    reader.load_withdrawn().await?;
    let root = reader.admit(root).await?;
    let ingredients = Ingredients::from_list(reader.ingredients().await?);
    let rules = read_pack_rules(&root, &ingredients)?;
    let mut closure = Closure::default();
    closure.walk(&mut reader, root).await?;
    for sourced in space.packs.iter().chain(space.overlays.iter()) {
        let seed = reader
            .admit(crate::held::Held::from_sourced(sourced.clone()))
            .await?;
        closure.walk(&mut reader, seed).await?;
    }
    Ok(closure
        .conflicts()
        .into_iter()
        .filter(|conflict| !never_together(conflict, &rules))
        .collect())
}
