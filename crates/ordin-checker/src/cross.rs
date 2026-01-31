//! Section D, steps 10 and 12: the cross-checks between the parts of one repository (port of
//! `CrossCheck`) and the ingest rules (port of the sync worker's steps). Pure: the union only.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use ordin_agent_model::{
    compare_versions, is_part_identifier, overlay_order, FixedPackEntry, PartKind, VocabularyAxis,
    VocabularySet,
};

use crate::body::strings;
use crate::context::Item;
use crate::finding::part_failure;
use crate::model::{Finding, Fix};

fn change(field: &str) -> Vec<Fix> {
    vec![Fix::ChangeItYourself {
        field: field.to_owned(),
    }]
}

fn fail(item: &Item, code: &str, field: &str, detail: String) -> Finding {
    part_failure(code, item.id(), item.name(), change(field), detail)
}

fn find<'a>(union: &'a [Item], kind: PartKind, id: &str) -> Option<&'a Item> {
    union.iter().find(|i| i.kind == kind && i.id() == id)
}

/// True when `version` satisfies `range`: empty = any, `^x.y.z` = same major and at least x.y.z,
/// otherwise the exact version.
pub(crate) fn admits(range: &str, version: &str) -> bool {
    if range.is_empty() {
        return true;
    }
    match range.strip_prefix('^') {
        Some(floor) => {
            let major = |v: &str| v.split('.').next().map(str::to_owned);
            major(floor) == major(version)
                && compare_versions(version, floor).is_ok_and(|o| o != Ordering::Less)
        }
        None => range == version,
    }
}

/// True when a part that may name the agents it applies to admits `workspace`.
fn applies_to_workspace(part: &Item, workspace: &Item) -> bool {
    match part.manifest.applies_to() {
        Ok(pins) if !pins.is_empty() => pins.iter().any(|pin| {
            pin.id == workspace.id() && admits(&pin.range, &workspace.manifest.metadata.version)
        }),
        _ => true,
    }
}

/// Control loops: a loop's modes stay inside the agent's allowed operating modes.
fn control_loops(union: &[Item], out: &mut Vec<Finding>) {
    for workspace in union
        .iter()
        .filter(|i| i.kind == PartKind::WorkspaceProfile)
    {
        let allowed = workspace.manifest.allowed_operating_modes();
        if allowed.is_empty() {
            continue;
        }
        for loop_id in strings(&workspace.manifest, "controlLoop") {
            let Some(control) = find(union, PartKind::ControlLoop, &loop_id) else {
                continue;
            };
            let Ok(Some(spec)) = control.manifest.control_loop() else {
                continue;
            };
            for mode in spec.modes.iter().filter(|m| !allowed.contains(m)) {
                out.push(fail(
                    workspace,
                    "sync_control_loop_mode_escape",
                    "allowedOperatingModes",
                    format!(
                        "control loop {loop_id} enters mode {mode}, which {} does not allow",
                        workspace.id()
                    ),
                ));
            }
        }
    }
}

/// appliesTo: each pin names a WorkspaceProfile of the repository at a version its range admits.
fn applies_to(union: &[Item], out: &mut Vec<Finding>) {
    for item in union {
        let pins = match item.manifest.applies_to() {
            Ok(pins) => pins,
            Err(error) => {
                out.push(fail(
                    item,
                    "sync_applies_to_malformed",
                    "appliesTo",
                    format!("appliesTo is malformed ({})", error.code()),
                ));
                continue;
            }
        };
        for pin in pins {
            let workspaces: Vec<&Item> = union
                .iter()
                .filter(|i| i.kind == PartKind::WorkspaceProfile && i.id() == pin.id)
                .collect();
            if workspaces.is_empty() {
                out.push(fail(
                    item,
                    "sync_applies_to_unbound",
                    "appliesTo",
                    format!("appliesTo names {}, which is not an agent here", pin.id),
                ));
            } else if !workspaces
                .iter()
                .any(|w| admits(&pin.range, &w.manifest.metadata.version))
            {
                out.push(fail(
                    item,
                    "sync_applies_to_unsatisfiable",
                    "appliesTo",
                    format!("no version of {} satisfies {:?}", pin.id, pin.range),
                ));
            }
        }
    }
}

/// The packs an agent offers by identifier: selectable, fixed and named by a required rule.
pub(crate) fn offered_pack_ids(workspace: &Item) -> Vec<String> {
    let manifest = &workspace.manifest;
    let mut ids: Vec<String> = manifest
        .selectable_practice_packs()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.id)
        .collect();
    for entry in manifest.fixed_practice_packs().unwrap_or_default() {
        if let FixedPackEntry::Pack(id) = entry {
            ids.push(id);
        }
    }
    for rule in manifest.required_practice_packs().unwrap_or_default() {
        ids.extend(rule.ids);
    }
    ids
}

/// Profile pack offers: every offered pack admits the agent.
fn pack_offers(union: &[Item], out: &mut Vec<Finding>) {
    for workspace in union
        .iter()
        .filter(|i| i.kind == PartKind::WorkspaceProfile)
    {
        for id in offered_pack_ids(workspace) {
            if let Some(pack) = find(union, PartKind::PracticePack, &id) {
                if !applies_to_workspace(pack, workspace) {
                    out.push(fail(
                        workspace,
                        "sync_pack_not_applicable",
                        "selectablePracticePacks",
                        format!("practice pack {id} does not apply to {}", workspace.id()),
                    ));
                }
            }
        }
    }
}

/// The overlays an agent offers on each axis: (axis, available identifiers, default).
pub(crate) fn overlay_offers(workspace: &Item) -> Vec<(PartKind, Vec<String>, Option<String>)> {
    overlay_order()
        .iter()
        .filter_map(|axis| {
            workspace
                .manifest
                .overlay_offer(*axis)
                .ok()
                .flatten()
                .map(|offer| (*axis, offer.available, offer.default))
        })
        .collect()
}

/// Profile overlay offers: the default is offered, and every offered overlay admits the agent.
fn overlay_offer_checks(union: &[Item], out: &mut Vec<Finding>) {
    for workspace in union
        .iter()
        .filter(|i| i.kind == PartKind::WorkspaceProfile)
    {
        for (axis, available, default) in overlay_offers(workspace) {
            if let Some(default) = default.filter(|d| !available.contains(d)) {
                out.push(fail(
                    workspace,
                    "sync_overlay_not_applicable",
                    "governanceMetadata",
                    format!("the default overlay {default} is not among those offered"),
                ));
            }
            for id in &available {
                if let Some(overlay) = find(union, axis, id) {
                    if !applies_to_workspace(overlay, workspace) {
                        out.push(fail(
                            workspace,
                            "sync_overlay_not_applicable",
                            "governanceMetadata",
                            format!("overlay {id} does not apply to {}", workspace.id()),
                        ));
                    }
                }
            }
        }
    }
}

/// True when the Evaluation `id` is a member of the set the publish gate `gate` names: the gate
/// itself, or an identifier under it with the separator of the gate's spelling (`:` for the URN
/// spelling, `/` for the `@` spelling). A shared prefix without the separator is not membership.
fn in_gate_set(gate: &str, id: &str) -> bool {
    let separator = if gate.starts_with('@') { '/' } else { ':' };
    id == gate
        || id
            .strip_prefix(gate)
            .is_some_and(|rest| rest.starts_with(separator))
}

/// Publish gate present: a gate naming a part names a set of evaluations, and the set has at least
/// one Evaluation of the repository in it.
///
/// `repository_evaluations` holds, by the workspace's id, the Evaluation identifiers of the
/// repository the workspace was read from (Checker.md CHK-PG-1): the checks of one agent see only
/// its closure, which never holds the evaluations a publish gate names.
// spec: ordin-desktop/components/Checker#sync_publish_gate_missing
// spec: ordin-desktop/components/Checker#CHK-PG-1
fn publish_gate(
    union: &[Item],
    repository_evaluations: &BTreeMap<String, Vec<String>>,
    out: &mut Vec<Finding>,
) {
    for workspace in union
        .iter()
        .filter(|i| i.kind == PartKind::WorkspaceProfile)
    {
        if let Some(gate) = workspace.manifest.publish_gate() {
            let gate = gate.trim();
            let held = repository_evaluations
                .get(workspace.id())
                .is_some_and(|ids| ids.iter().any(|id| in_gate_set(gate, id)));
            if is_part_identifier(gate)
                && !held
                && !union
                    .iter()
                    .any(|i| i.kind == PartKind::Evaluation && in_gate_set(gate, i.id()))
            {
                out.push(fail(
                    workspace,
                    "sync_publish_gate_missing",
                    "governanceMetadata.publishGate",
                    format!("the publish gate {gate} names no evaluation here"),
                ));
            }
        }
    }
}

/// Crew references and crew (CrewProfile parts are read and checked, never offered).
// `sync_crew_empty` (a crew with no `members`), `sync_crew_member_unresolved` (a member that is not an
// agent of the repository) and `sync_crew_loop_override_invalid` (a crew `controlLoop` that is not a
// control loop of the repository).
fn crews(union: &[Item], out: &mut Vec<Finding>) {
    for crew in union.iter().filter(|i| i.kind == PartKind::CrewProfile) {
        let members = strings(&crew.manifest, "members");
        if members.is_empty() {
            out.push(fail(
                crew,
                "sync_crew_empty",
                "members",
                "the crew has no members".to_owned(),
            ));
        }
        for member in members {
            if find(union, PartKind::WorkspaceProfile, &member).is_none() {
                out.push(fail(
                    crew,
                    "sync_crew_member_unresolved",
                    "members",
                    format!("crew member {member} is not an agent here"),
                ));
            }
        }
        for loop_id in strings(&crew.manifest, "controlLoop") {
            if find(union, PartKind::ControlLoop, &loop_id).is_none() {
                out.push(fail(
                    crew,
                    "sync_crew_loop_override_invalid",
                    "controlLoop",
                    format!("the crew's control loop {loop_id} is not a control loop here"),
                ));
            }
        }
    }
}

/// Memory: one memory per agent.
fn memory(union: &[Item], out: &mut Vec<Finding>) {
    for workspace in union
        .iter()
        .filter(|i| i.kind == PartKind::WorkspaceProfile)
    {
        let memories = strings(&workspace.manifest, "memory");
        if memories.len() > 1 {
            out.push(fail(
                workspace,
                "memory_cardinality",
                "memory",
                format!("the agent names {} memories; at most one", memories.len()),
            ));
        }
    }
}

/// Step 10 over one tree.
pub(crate) fn cross_checks(
    union: &[Item],
    repository_evaluations: &BTreeMap<String, Vec<String>>,
) -> Vec<Finding> {
    let mut out = Vec::new();
    control_loops(union, &mut out);
    applies_to(union, &mut out);
    pack_offers(union, &mut out);
    overlay_offer_checks(union, &mut out);
    publish_gate(union, repository_evaluations, &mut out);
    crews(union, &mut out);
    memory(union, &mut out);
    out
}

/// Step 12: the ingest rules.
// The Guideline's target kind is read from the body key `targetKind`.
pub(crate) fn ingest_rules(union: &[Item], vocabulary: &VocabularySet) -> Vec<Finding> {
    let mut out = Vec::new();
    for item in union {
        if let Some(target) = &item.manifest.metadata.supersedes {
            if !union.iter().any(|i| i.id() == target) {
                out.push(fail(
                    item,
                    "sync_unknown_supersedes_target",
                    "metadata.supersedes",
                    format!("metadata.supersedes names {target}, which is not a part here"),
                ));
            }
        }
    }
    let mut guidelines: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
    for item in union.iter().filter(|i| i.kind == PartKind::Guideline) {
        for target in strings(&item.manifest, "targetKind") {
            guidelines.entry(target).or_default().push(item);
        }
    }
    for (target, items) in guidelines {
        for item in items.iter().skip(1) {
            out.push(fail(
                item,
                "sync_duplicate_guideline_target_kind",
                "targetKind",
                format!("a second guideline for the kind {target}"),
            ));
        }
    }
    // Stage one already checks the descriptive metadata of WorkspaceProfiles and PracticePacks.
    for item in union
        .iter()
        .filter(|i| !matches!(i.kind, PartKind::WorkspaceProfile | PartKind::PracticePack))
    {
        let metadata = &item.manifest.metadata;
        let mut cited: Vec<(VocabularyAxis, &str)> = Vec::new();
        if !metadata.category.is_empty() {
            cited.push((VocabularyAxis::Category, &metadata.category));
        }
        if !metadata.class.is_empty() {
            cited.push((VocabularyAxis::PackClass, &metadata.class));
        }
        cited.extend(
            metadata
                .audience
                .roles
                .iter()
                .map(|r| (VocabularyAxis::AudienceRole, r.as_str())),
        );
        for (axis, code) in cited {
            if !vocabulary.has(axis, code) {
                out.push(fail(
                    item,
                    "sync_unknown_vocabulary_term",
                    "metadata",
                    format!("the part cites {code:?}, which is not in the vocabulary"),
                ));
            }
        }
    }
    out
}

/// The key a finding is compared on for "new findings only" (step 10).
pub(crate) fn finding_key(f: &Finding) -> (String, Option<String>, String) {
    (f.code.clone(), f.part_identifier.clone(), f.detail.clone())
}

#[cfg(test)]
mod publish_gate_tests {
    //! Step 10's publish-gate cross-check (Checker.md, amendment 2026-01-26). Under
    //! `testdata/publish-gate/`, `coordinator-workspace-profile.yaml` is a copy of Ordin's
    //! coordinator from the bundled agents repository; the agent maker's workspace, the
    //! ci-gate and the evaluation are small parts written for this test.

    use std::collections::BTreeMap;
    use std::path::Path;

    use ordin_agent_model::{parse_manifest, PartKind, VocabularySet};

    use super::{in_gate_set, publish_gate};
    use crate::context::Item;
    use crate::model::{Finding, Fix};

    fn item(path: &str, yaml: &str) -> Item {
        let manifest = parse_manifest(path, yaml.as_bytes())
            .expect("the fixture parses")
            .expect("the fixture is a manifest");
        let kind = manifest
            .kind(&VocabularySet::default())
            .expect("the fixture's kind is known");
        Item {
            kind,
            manifest,
            path: path.to_owned(),
            files: None,
            changed: false,
            sourced: None,
        }
    }

    fn fixture(name: &str) -> Item {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("publish-gate")
            .join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("fixture {} cannot be read: {e}", path.display()));
        item(name, &text)
    }

    fn workspace(id: &str, gate: &str) -> Item {
        item(
            "workspace-profile.yaml",
            &format!(
                "apiVersion: ai.enterprise/v1\nkind: workspace-profile\nmetadata:\n  id: \"{id}\"\n  version: 1.0.0\ngovernanceMetadata:\n  publishGate: \"{gate}\"\n"
            ),
        )
    }

    fn evaluation(id: &str) -> Item {
        item(
            "evaluation.yaml",
            &format!(
                "apiVersion: ai.enterprise/v1\nkind: evaluation\nmetadata:\n  id: \"{id}\"\n  version: 1.0.0\n"
            ),
        )
    }

    fn check(union: &[Item]) -> Vec<Finding> {
        let mut out = Vec::new();
        publish_gate(union, &BTreeMap::new(), &mut out);
        out
    }

    const WORKSPACE: &str = "urn:ordin:default:workspaces:w";

    #[test]
    fn bundled_advisor_and_coordinator_pass_the_publish_gate() {
        let union = vec![
            fixture("advisor-workspace-profile.yaml"),
            fixture("coordinator-workspace-profile.yaml"),
            fixture("ci-gate.yaml"),
            fixture("observe-refuses-edit.yaml"),
        ];
        for ws in &union[..2] {
            assert_eq!(ws.kind, PartKind::WorkspaceProfile);
            assert_eq!(
                ws.manifest.publish_gate().as_deref(),
                Some("urn:ordin:default:evals")
            );
        }
        assert_eq!(
            union[2].kind,
            PartKind::Evaluation,
            "ci-gate is an Evaluation"
        );
        // No evaluation carries the gate's name exactly: only the set reading admits them.
        assert!(union[2..]
            .iter()
            .all(|e| e.id() != "urn:ordin:default:evals"));
        assert_eq!(check(&union), Vec::new());
    }

    #[test]
    fn a_gate_naming_a_set_is_met_by_an_evaluation_under_it() {
        let union = vec![
            workspace(WORKSPACE, "urn:ordin:default:evals"),
            evaluation("urn:ordin:default:evals:ci-gate"),
        ];
        assert_eq!(check(&union), Vec::new());
    }

    #[test]
    fn a_gate_naming_one_evaluation_exactly_is_met() {
        let union = vec![
            workspace(WORKSPACE, "urn:ordin:default:evals:ci-gate"),
            evaluation("urn:ordin:default:evals:ci-gate"),
        ];
        assert_eq!(check(&union), Vec::new());
    }

    #[test]
    fn a_gate_naming_an_empty_set_fails() {
        let union = vec![
            workspace(WORKSPACE, "urn:ordin:default:evals:ghost"),
            evaluation("urn:ordin:default:evals:ci-gate"),
        ];
        let found = check(&union);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "sync_publish_gate_missing");
        assert_eq!(found[0].part_identifier.as_deref(), Some(WORKSPACE));
        assert_eq!(
            found[0].fixes,
            vec![Fix::ChangeItYourself {
                field: "governanceMetadata.publishGate".to_owned()
            }]
        );
    }

    #[test]
    fn a_shared_name_prefix_is_not_membership() {
        let union = vec![
            workspace(WORKSPACE, "urn:ordin:default:evals"),
            evaluation("urn:ordin:default:evalsuite:x"),
        ];
        let codes: Vec<_> = check(&union).into_iter().map(|f| f.code).collect();
        assert_eq!(codes, vec!["sync_publish_gate_missing".to_owned()]);
    }

    // Scenario: Happy path — a library item's gate is met by its repository's evaluation (CHK-PG-1)
    #[test]
    fn a_gate_is_met_by_an_evaluation_of_the_workspaces_repository() {
        let union = vec![workspace(WORKSPACE, "urn:ordin:default:evals")];
        let held = BTreeMap::from([(
            WORKSPACE.to_owned(),
            vec!["urn:ordin:default:evals:ci-gate".to_owned()],
        )]);
        let mut out = Vec::new();
        publish_gate(&union, &held, &mut out);
        assert_eq!(out, Vec::new());
    }

    // Scenario: Error path — the repository has no evaluation in the set (CHK-PG-1)
    #[test]
    fn a_gate_is_not_met_by_an_evaluation_outside_the_set() {
        let union = vec![workspace(WORKSPACE, "urn:ordin:default:evals")];
        let held = BTreeMap::from([(
            WORKSPACE.to_owned(),
            vec!["urn:ordin:default:other:x".to_owned()],
        )]);
        let mut out = Vec::new();
        publish_gate(&union, &held, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, "sync_publish_gate_missing");
    }

    // Scenario: the evaluations of another workspace's repository do not count (CHK-PG-1)
    #[test]
    fn another_workspaces_repository_does_not_count() {
        let union = vec![workspace(WORKSPACE, "urn:ordin:default:evals")];
        let held = BTreeMap::from([(
            "urn:ordin:default:workspaces:other".to_owned(),
            vec!["urn:ordin:default:evals:ci-gate".to_owned()],
        )]);
        let mut out = Vec::new();
        publish_gate(&union, &held, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn membership_follows_the_spelling_of_the_gate() {
        let cases = [
            ("urn:ordin:default:evals", "urn:ordin:default:evals", true),
            (
                "urn:ordin:default:evals",
                "urn:ordin:default:evals:a:b",
                true,
            ),
            ("urn:ordin:default:evals", "urn:ordin:default:evalsx", false),
            (
                "urn:ordin:default:evals",
                "urn:ordin:default:evals/a",
                false,
            ),
            ("@default/evals", "@default/evals/a", true),
            ("@default/evals", "@default/evals:a", false),
            ("@default/evals", "@default/evalsx", false),
        ];
        for (gate, id, expected) in cases {
            assert_eq!(in_gate_set(gate, id), expected, "{gate} / {id}");
        }
    }
}
