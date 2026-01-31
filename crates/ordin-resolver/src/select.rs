//! Practice-pack and overlay selection (ResolverAndCompiler.md *Internal Logic* 4 and 5).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use ordin_agent_model::{
    compare_versions, kind_info, overlay_order, slug_from_id, strip_kind_dir_prefix, ChosenBy,
    FixedPackEntry, PartKind, PendingPackChoice,
};

use crate::error::{manifest_invalid, ResolveError};
use crate::held::Held;
use crate::log;
use crate::request::ResolveRequest;

/// The packs and overlays visible through the scope, latest non-draft version of each identifier.
pub(crate) struct Ingredients {
    pub packs: BTreeMap<String, Held>,
    pub overlays: BTreeMap<(PartKind, String), Held>,
}

impl Ingredients {
    pub(crate) fn from_list(list: Vec<Held>) -> Ingredients {
        let mut packs = BTreeMap::new();
        let mut overlays = BTreeMap::new();
        for held in list {
            match held.kind() {
                PartKind::PracticePack => {
                    packs.insert(held.id().to_owned(), held);
                }
                kind if overlay_order().contains(&kind) => {
                    overlays.insert((kind, held.id().to_owned()), held);
                }
                _ => {}
            }
        }
        Ingredients { packs, overlays }
    }
}

/// One `requiredPracticePacks` rule with its candidates.
#[derive(Debug, Clone)]
pub(crate) struct RuleView {
    pub index: u32,
    pub at_least: u32,
    pub class: Option<String>,
    pub ids: Vec<String>,
    /// Sorted pack identifiers the rule allows.
    pub candidates: Vec<String>,
}

/// What the root says about practice packs.
pub(crate) struct PackRules {
    pub rules: Vec<RuleView>,
    /// Selectable packs that resolve, in authored order.
    pub selectable: Vec<String>,
    pub fixed: Vec<FixedPackEntry>,
}

/// Step 4: read the three pack fields of the root and work out each rule's candidates.
pub(crate) fn read_pack_rules(
    root: &Held,
    ingredients: &Ingredients,
) -> Result<PackRules, ResolveError> {
    let invalid = |error| manifest_invalid(root.id(), &error);
    let manifest = root.manifest();
    let required = manifest.required_practice_packs().map_err(invalid)?;
    let listed = manifest.selectable_practice_packs().map_err(invalid)?;
    let fixed = manifest.fixed_practice_packs().map_err(invalid)?;
    let has_selectable_list = !listed.is_empty();
    let mut selectable: Vec<String> = Vec::new();
    for pack in &listed {
        if ingredients.packs.contains_key(&pack.id) {
            if !selectable.contains(&pack.id) {
                selectable.push(pack.id.clone());
            }
        } else {
            log::pack_candidate_missing(&pack.id);
        }
    }
    let class_of = |id: &String| {
        ingredients
            .packs
            .get(id)
            .map(|pack| pack.manifest().metadata.class.clone())
    };
    let mut rules = Vec::new();
    for (index, rule) in required.into_iter().enumerate() {
        let mut candidates: Vec<String> = if !rule.ids.is_empty() {
            let mut kept = Vec::new();
            for id in &rule.ids {
                if ingredients.packs.contains_key(id) {
                    kept.push(id.clone());
                } else {
                    log::pack_candidate_missing(id);
                }
            }
            kept
        } else if let Some(class) = &rule.class {
            let pool: Vec<String> = if has_selectable_list {
                selectable.clone()
            } else {
                ingredients.packs.keys().cloned().collect()
            };
            pool.into_iter()
                .filter(|id| class_of(id).as_deref() == Some(class.as_str()))
                .collect()
        } else {
            Vec::new()
        };
        candidates.sort();
        candidates.dedup();
        rules.push(RuleView {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            at_least: rule.at_least,
            class: rule.class,
            ids: rule.ids,
            candidates,
        });
    }
    Ok(PackRules {
        rules,
        selectable,
        fixed,
    })
}

/// True when a pack's `metadata.audience.languages` equals, ignoring ASCII case, a workspace language.
fn matches_workspace(pack: &Held, request: &ResolveRequest) -> bool {
    pack.manifest()
        .metadata
        .audience
        .languages
        .iter()
        .any(|language| {
            request
                .workspace_languages
                .iter()
                .any(|workspace| workspace.eq_ignore_ascii_case(language))
        })
}

/// An `ask` entry answers a rule when it names the rule's class, or the same set of identifiers.
fn ask_covers(entry: &FixedPackEntry, rule: &RuleView) -> bool {
    match entry {
        FixedPackEntry::Pack(_) => false,
        FixedPackEntry::Ask { class, ids } => {
            let by_class = rule.class.is_some() && class == &rule.class;
            let by_ids = !rule.ids.is_empty()
                && !ids.is_empty()
                && ids.iter().collect::<BTreeSet<_>>() == rule.ids.iter().collect::<BTreeSet<_>>();
            by_class || by_ids
        }
    }
}

/// The packs chosen, and the rules left to ask.
pub(crate) type PackSelection = (Vec<(Held, ChosenBy)>, Vec<PendingPackChoice>);

/// Step 4.3 and 4.4: the packs chosen and the rules left to ask.
pub(crate) fn select_packs(
    rules: &PackRules,
    ingredients: &Ingredients,
    request: &ResolveRequest,
) -> Result<PackSelection, ResolveError> {
    let mut chosen: BTreeMap<String, ChosenBy> = BTreeMap::new();
    let mut pending = Vec::new();
    let in_rules: BTreeSet<&String> = rules.rules.iter().flat_map(|r| &r.candidates).collect();

    for id in &request.pack_changes {
        if !in_rules.contains(id) && !rules.selectable.contains(id) {
            return Err(ResolveError::PackNotAllowed { id: id.clone() });
        }
    }

    for rule in &rules.rules {
        if let Some(answer) = request
            .pack_answers
            .iter()
            .find(|a| a.rule_index == rule.index)
        {
            if let Some(id) = answer
                .pack_ids
                .iter()
                .find(|id| !rule.candidates.contains(id))
            {
                return Err(ResolveError::PackNotAllowed { id: id.clone() });
            }
            for id in &answer.pack_ids {
                chosen.entry(id.clone()).or_insert(ChosenBy::PersonAnswer);
            }
            continue;
        }
        let changes: BTreeSet<&String> = request
            .pack_changes
            .iter()
            .filter(|id| rule.candidates.contains(id))
            .collect();
        if !changes.is_empty() {
            for id in changes {
                chosen.entry(id.clone()).or_insert(ChosenBy::PersonChange);
            }
            continue;
        }
        let needed = usize::try_from(rule.at_least).unwrap_or(usize::MAX);
        let mut picked: Vec<(String, ChosenBy)> = rule
            .candidates
            .iter()
            .filter(|id| {
                ingredients
                    .packs
                    .get(*id)
                    .is_some_and(|pack| matches_workspace(pack, request))
            })
            .take(needed)
            .map(|id| (id.clone(), ChosenBy::WorkspaceCode))
            .collect();
        if picked.len() < needed {
            for entry in &rules.fixed {
                if picked.len() >= needed {
                    break;
                }
                if let FixedPackEntry::Pack(id) = entry {
                    if rule.candidates.contains(id) && !picked.iter().any(|(p, _)| p == id) {
                        picked.push((id.clone(), ChosenBy::MakerFixed));
                    }
                }
            }
        }
        if picked.len() < needed {
            if rules.fixed.iter().any(|entry| ask_covers(entry, rule)) {
                pending.push(PendingPackChoice {
                    rule_index: rule.index,
                    at_least: rule.at_least,
                    class: rule.class.clone(),
                    choices: rule.candidates.clone(),
                });
            } else {
                return Err(ResolveError::RequiredPackUnsettled {
                    rule_index: rule.index,
                    class: rule.class.clone(),
                });
            }
        }
        for (id, by) in picked {
            chosen.entry(id).or_insert(by);
        }
    }

    // Optional packs: selectable and not a candidate of any rule.
    for id in &rules.selectable {
        if in_rules.contains(id) {
            continue;
        }
        let Some(pack) = ingredients.packs.get(id) else {
            continue;
        };
        let by = if request.pack_changes.contains(id) {
            Some(ChosenBy::PersonChange)
        } else if matches_workspace(pack, request) {
            Some(ChosenBy::WorkspaceCode)
        } else if rules
            .fixed
            .iter()
            .any(|entry| matches!(entry, FixedPackEntry::Pack(fixed) if fixed == id))
        {
            Some(ChosenBy::MakerFixed)
        } else {
            None
        };
        if let Some(by) = by {
            chosen.entry(id.clone()).or_insert(by);
        }
    }

    let packs = chosen
        .into_iter()
        .filter_map(|(id, by)| ingredients.packs.get(&id).map(|pack| (pack.clone(), by)))
        .collect();
    Ok((packs, pending))
}

/// The axes whose overlays an agent offers and the resolver applies. Brand and jurisdiction overlays
/// are read but not offered; handoff overlays are bound from outside through `appliesTo`.
fn offered_axes() -> impl Iterator<Item = PartKind> {
    overlay_order().iter().copied().filter(|axis| {
        !matches!(
            axis,
            PartKind::HandoffOverlay | PartKind::BrandOverlay | PartKind::JurisdictionOverlay
        )
    })
}

/// The last segment of an overlay identifier after the kind directory (`en-GB` of `…:language:en-GB`).
fn last_segment(id: &str) -> String {
    strip_kind_dir_prefix(id)
        .rsplit([':', '/'])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The language overlay for an interface language: an exact last segment, then the primary subtag.
fn language_match<'a>(available: &'a [Held], interface_language: &str) -> Option<&'a Held> {
    let exact = available
        .iter()
        .find(|h| last_segment(h.id()).eq_ignore_ascii_case(interface_language));
    if exact.is_some() {
        return exact;
    }
    let primary = interface_language.split('-').next().unwrap_or_default();
    available
        .iter()
        .find(|h| last_segment(h.id()).eq_ignore_ascii_case(primary))
}

/// Whether `version` satisfies a pin's range: empty (any), exact, or caret (same major, and not lower).
fn range_allows(version: &str, range: &str) -> bool {
    if range.is_empty() {
        return true;
    }
    match range.strip_prefix('^') {
        Some(base) => {
            let major = |v: &str| v.split('.').next().map(str::to_owned);
            major(version) == major(base)
                && matches!(
                    compare_versions(version, base),
                    Ok(Ordering::Equal | Ordering::Greater)
                )
        }
        None => matches!(compare_versions(version, range), Ok(Ordering::Equal)),
    }
}

/// Step 5: the overlays chosen, in application order (axis tier, then identifier).
pub(crate) fn select_overlays(
    root: &Held,
    ingredients: &Ingredients,
    request: &ResolveRequest,
) -> Result<Vec<(Held, ChosenBy)>, ResolveError> {
    let invalid = |error| manifest_invalid(root.id(), &error);
    let manifest = root.manifest();
    let axes: Vec<PartKind> = offered_axes().collect();

    // Changes: each must be an overlay of an offered axis, listed in that axis's `available`.
    let mut offers = BTreeMap::new();
    for axis in &axes {
        offers.insert(*axis, manifest.overlay_offer(*axis).map_err(invalid)?);
    }
    let mut changes: BTreeMap<PartKind, Vec<String>> = BTreeMap::new();
    for id in &request.overlay_changes {
        let kind = ingredients
            .overlays
            .keys()
            .find(|(_, held_id)| held_id == id)
            .map(|(kind, _)| *kind);
        let allowed = kind.filter(|kind| {
            offers
                .get(kind)
                .and_then(Option::as_ref)
                .is_some_and(|offer| offer.available.contains(id))
        });
        let Some(kind) = allowed else {
            return Err(ResolveError::OverlayNotAllowed { id: id.clone() });
        };
        let list = changes.entry(kind).or_default();
        if !list.contains(id) {
            list.push(id.clone());
        }
    }

    let mut selected: Vec<(Held, ChosenBy)> = Vec::new();
    for axis in &axes {
        let offer = offers.get(axis).and_then(Option::as_ref);
        let available: Vec<Held> = offer
            .map(|offer| {
                offer
                    .available
                    .iter()
                    .filter_map(|id| {
                        let found = ingredients.overlays.get(&(*axis, id.clone()));
                        if found.is_none() {
                            log::pack_candidate_missing(id);
                        }
                        found.cloned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut on_axis: Vec<(Held, ChosenBy)> = Vec::new();
        if let Some(ids) = changes.get(axis) {
            for id in ids {
                if let Some(held) = ingredients.overlays.get(&(*axis, id.clone())) {
                    on_axis.push((held.clone(), ChosenBy::PersonChange));
                }
            }
        } else if *axis == PartKind::LanguageOverlay
            && language_match(&available, &request.interface_language).is_some()
        {
            if let Some(held) = language_match(&available, &request.interface_language) {
                on_axis.push((held.clone(), ChosenBy::InterfaceLanguage));
            }
        } else if let Some(default) = offer.and_then(|o| o.default.as_ref()) {
            match ingredients.overlays.get(&(*axis, default.clone())) {
                Some(held) => on_axis.push((held.clone(), ChosenBy::AgentDefault)),
                None => log::pack_candidate_missing(default),
            }
        }
        if on_axis.len() > 1 && kind_info(*axis).exactly_one {
            return Err(ResolveError::OverlayCardinality { axis: *axis });
        }
        selected.extend(on_axis);
    }

    // Required overlays: an axis with a non-empty `anyOf` needs a selected overlay with a matching slug.
    // Brand, jurisdiction and handoff overlays are never offered or applied by this component, so
    // only the offered axes can be unsatisfied.
    let required = manifest.required_overlays().map_err(invalid)?;
    for (axis, any_of) in required {
        if any_of.is_empty() || !axes.contains(&axis) {
            continue;
        }
        let satisfied = selected.iter().any(|(held, _)| {
            held.kind() == axis
                && any_of
                    .iter()
                    .any(|entry| slug_from_id(entry) == slug_from_id(held.id()))
        });
        if !satisfied {
            return Err(ResolveError::RequiredOverlayMissing { axis });
        }
    }

    // Applicability: an overlay with `appliesTo` pins applies only when a pin names this agent.
    let root_slug = manifest.slug();
    for (held, _) in &selected {
        let pins = held
            .manifest()
            .applies_to()
            .map_err(|error| manifest_invalid(held.id(), &error))?;
        if pins.is_empty() {
            continue;
        }
        let applies = pins.iter().any(|pin| {
            slug_from_id(&pin.id) == root_slug && range_allows(root.version(), &pin.range)
        });
        if !applies {
            return Err(ResolveError::OverlayNotApplicable {
                overlay: held.id().to_owned(),
            });
        }
    }

    let tier = |kind: PartKind| {
        overlay_order()
            .iter()
            .position(|k| *k == kind)
            .unwrap_or(usize::MAX)
    };
    selected.sort_by(|(a, _), (b, _)| {
        tier(a.kind())
            .cmp(&tier(b.kind()))
            .then_with(|| a.id().cmp(b.id()))
    });
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert!(range_allows("1.2.0", ""));
        assert!(range_allows("1.0.0", "1.0.0"));
        assert!(!range_allows("1.0.1", "1.0.0"));
        assert!(range_allows("1.4.0", "^1.2.0"));
        assert!(!range_allows("1.1.0", "^1.2.0"));
        assert!(!range_allows("2.0.0", "^1.2.0"));
        assert!(!range_allows("x", "^1.2.0"));
    }

    #[test]
    fn last_segments() {
        assert_eq!(
            last_segment("urn:ordin:default:overlays:language:en-GB"),
            "en-GB"
        );
        assert_eq!(last_segment("@default/overlays/language/fr"), "fr");
    }
}
