//! The resolution core: root, withdrawn list, selections, closure, gates, plan hash, the lock-file
//! re-resolution, and the `AGENTS.md` text (ResolverAndCompiler.md *Internal Logic* 1 to 15, 22, 22b to
//! 22d).

use std::collections::BTreeSet;
use std::time::Instant;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use ordin_agent_model::{
    canonical_json, kind_info, markdown_body, prefixed_digest, slug_from_id, ChosenBy, LoadingMode,
    LockFile, PartKind, PartRef, PartSource, PartStatus, ResolutionPlan, ResolvedToolPolicy,
    SelectedPart, Selections, SourceScope, WithdrawnListEntry, LOCK_FILE_SCHEMA,
};

use crate::closure::Closure;
use crate::error::{manifest_invalid, ResolveError};
use crate::held::{files_of, Held, Reader};
use crate::log;
use crate::policy::compose_tool_policy;
use crate::request::{CompiledInstructions, ResolveRequest};
use crate::select::{read_pack_rules, select_overlays, select_packs, Ingredients};

/// A plan with the parts it was built from, aligned with `plan.resolved`.
pub(crate) struct Resolved {
    pub plan: ResolutionPlan,
    pub held: Vec<Held>,
    pub root: Held,
}

/// `sha256:<hex>` of bytes.
pub(crate) fn sha256_prefixed(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    prefixed_digest(&ordin_agent_model::Sha256Hex::from_digest(&digest))
}

/// SHA-256 hexadecimal of bytes, without a prefix.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    ordin_agent_model::Sha256Hex::from_digest(&digest).0
}

/// Step 14: the plan hash covers the plan with `plan_hash = ""` and `tool_policy = None`.
pub(crate) fn compute_plan_hash(plan: &ResolutionPlan) -> String {
    let mut bare = plan.clone();
    bare.plan_hash = String::new();
    bare.tool_policy = None;
    let value = serde_json::to_value(&bare).unwrap_or(Value::Null);
    sha256_prefixed(&canonical_json(&value))
}

/// The harness as the plan and lock file spell it (`claude_code`, `copilot_cli`).
// spec: ordin-desktop/components/ResolverAndCompiler#SA-RECON-03
fn harness_name(request: &ResolveRequest) -> String {
    match serde_json::to_value(request.target_harness) {
        Ok(Value::String(name)) => name,
        _ => String::new(),
    }
}

/// Step 2's snapshot: the entries sorted, each as `{kind, identifier, version, contentHash}`.
// The sort is by part identifier, then kind and version, because the entry's own row id differs
// between sources for equal content.
pub(crate) fn withdrawn_snapshot(entries: &[WithdrawnListEntry]) -> String {
    let mut sorted: Vec<&WithdrawnListEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        a.part_identifier
            .cmp(&b.part_identifier)
            .then_with(|| a.part_kind.cmp(&b.part_kind))
            .then_with(|| a.part_version.cmp(&b.part_version))
    });
    let list: Vec<Value> = sorted
        .iter()
        .map(|entry| {
            json!({
                "kind": serde_json::to_value(entry.part_kind).unwrap_or(Value::Null),
                "identifier": entry.part_identifier,
                "version": entry.part_version,
                "contentHash": entry.part_content_digest.as_ref().map(prefixed_digest),
            })
        })
        .collect();
    sha256_prefixed(&canonical_json(&Value::Array(list)))
}

/// Step 1: the root, from a staged part or the source.
pub(crate) async fn load_root(
    reader: &mut Reader<'_>,
    request: &ResolveRequest,
) -> Result<Held, ResolveError> {
    let version = request.root_version.as_deref();
    let found = reader
        .lookup(Some(PartKind::WorkspaceProfile), &request.root_id, version)
        .await?;
    let root = match found {
        Some(root) => root,
        None => {
            // The identifier is looked up without a kind, and a part of another kind is refused as
            // not installable.
            return match reader.lookup(None, &request.root_id, None).await? {
                Some(other) => Err(ResolveError::NotInstallable { kind: other.kind() }),
                None => Err(ResolveError::RootNotFound {
                    id: request.root_id.clone(),
                }),
            };
        }
    };
    match root.status() {
        PartStatus::Approved => Ok(root),
        PartStatus::Draft if request.allow_draft_root => Ok(root),
        status => Err(ResolveError::RootNotApproved {
            id: root.id().to_owned(),
            status,
        }),
    }
}

/// Step 10: the closure gates.
fn closure_gates(root: &Held, held: &[Held]) -> Result<(), ResolveError> {
    let invalid = |part: &Held, error| manifest_invalid(part.id(), &error);
    let ids_of = |kind: PartKind| -> Vec<String> {
        held.iter()
            .filter(|h| h.kind() == kind)
            .map(|h| h.id().to_owned())
            .collect()
    };
    let memories = ids_of(PartKind::Memory);
    if memories.len() > 1 {
        return Err(ResolveError::MemoryCardinality { parts: memories });
    }
    let loops = ids_of(PartKind::ControlLoop);
    if loops.len() > 1 {
        return Err(ResolveError::LoopMultiple { parts: loops });
    }
    let rule = root
        .manifest()
        .required_control_loop()
        .map_err(|error| invalid(root, error))?;
    if rule.as_ref().is_some_and(|r| r.required) && loops.is_empty() {
        return Err(ResolveError::LoopMissing);
    }
    let Some(control_loop) = held.iter().find(|h| h.kind() == PartKind::ControlLoop) else {
        return Ok(());
    };
    if let Some(rule) = &rule {
        if !rule.ids.is_empty() && !rule.ids.iter().any(|id| id == control_loop.id()) {
            return Err(ResolveError::LoopNotPermitted {
                loop_id: control_loop.id().to_owned(),
            });
        }
    }
    let spec = control_loop
        .manifest()
        .control_loop()
        .map_err(|error| invalid(control_loop, error))?;
    if let Some(spec) = spec {
        // With no `allowedOperatingModes` on the root every mode of the loop is absent from the
        // allow-list, so the loop escapes.
        let allowed: BTreeSet<String> = root
            .manifest()
            .allowed_operating_modes()
            .iter()
            .map(|mode| slug_from_id(mode))
            .collect();
        if let Some(mode) = spec
            .modes
            .iter()
            .find(|mode| !allowed.contains(&slug_from_id(mode)))
        {
            return Err(ResolveError::LoopModeEscape {
                loop_id: control_loop.id().to_owned(),
                mode: mode.clone(),
            });
        }
    }
    Ok(())
}

/// Step 11: the lifecycle gate; returns the warnings in `held` order.
fn lifecycle_gate(root: &Held, held: &[Held]) -> Result<Vec<String>, ResolveError> {
    let mut warnings = Vec::new();
    for part in held.iter().filter(|h| h.key() != root.key()) {
        match part.status() {
            PartStatus::Approved => {}
            PartStatus::Deprecated => {
                warnings.push(deprecation_warning(part.id()));
                log::constituent_deprecated(part.id());
            }
            PartStatus::Draft => {
                if !part.staged {
                    return Err(ResolveError::DraftConstituent {
                        part: part.id().to_owned(),
                    });
                }
            }
        }
    }
    Ok(warnings)
}

fn deprecation_warning(id: &str) -> String {
    format!("constituent {id} is deprecated; do not add new uses")
}

/// Steps 1 to 15.
pub(crate) async fn resolve_core(
    source: &dyn PartSource,
    request: &ResolveRequest,
) -> Result<Resolved, ResolveError> {
    let started = Instant::now();
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
    let (packs, pending_choices) = select_packs(&rules, &ingredients, request)?;
    let overlays = select_overlays(&root, &ingredients, request)?;

    // Step 6: seeds are the root, the packs in identifier order, then the overlays in tier order.
    let mut pack_seeds: Vec<(Held, ChosenBy)> = Vec::new();
    for (pack, by) in packs {
        pack_seeds.push((reader.admit(pack).await?, by));
    }
    let mut overlay_seeds: Vec<(Held, ChosenBy)> = Vec::new();
    for (overlay, by) in overlays {
        overlay_seeds.push((reader.admit(overlay).await?, by));
    }

    let mut closure = Closure::default();
    closure.walk(&mut reader, root.clone()).await?;
    for (seed, _) in pack_seeds.iter().chain(overlay_seeds.iter()) {
        closure.walk(&mut reader, seed.clone()).await?;
    }
    let conflicts = closure.conflicts();
    if !conflicts.is_empty() {
        return Err(ResolveError::Conflict { conflicts });
    }
    let held = closure.into_parts();
    closure_gates(&root, &held)?;
    let warnings = lifecycle_gate(&root, &held)?;

    let selected = |list: &[(Held, ChosenBy)]| -> Vec<SelectedPart> {
        list.iter()
            .map(|(part, by)| SelectedPart {
                id: part.id().to_owned(),
                chosen_by: *by,
            })
            .collect()
    };
    let mut plan = ResolutionPlan {
        plan_hash: String::new(),
        root: root.part_ref(),
        target_harness: harness_name(request),
        overlay_order: overlay_seeds.iter().map(|(h, _)| h.part_ref()).collect(),
        practice_packs: pack_seeds.iter().map(|(h, _)| h.part_ref()).collect(),
        resolved: held.iter().map(Held::part_ref).collect(),
        selections: Selections {
            practice_packs: selected(&pack_seeds),
            overlays: selected(&overlay_seeds),
            pending_choices,
        },
        warnings,
        withdrawn_snapshot: withdrawn_snapshot(reader.withdrawn_entries()),
        tool_policy: None,
    };
    let policy = compose_tool_policy(&root, &held)?;
    plan.plan_hash = compute_plan_hash(&plan);
    plan.tool_policy = Some(policy);
    log::resolved(&plan, started.elapsed());
    Ok(Resolved { plan, held, root })
}

/// The lock file of a plan (step 18).
pub(crate) fn lock_of(plan: &ResolutionPlan) -> LockFile {
    LockFile {
        lockfile_schema: LOCK_FILE_SCHEMA.to_owned(),
        plan_hash: plan.plan_hash.clone(),
        target_harness: plan.target_harness.clone(),
        composition: plan.root.clone(),
        artefacts: plan.resolved.clone(),
        overlays_applied: plan.overlay_order.clone(),
        selections: plan.selections.clone(),
    }
}

/// Step 22: the plan a lock file names, each part checked against the source.
pub(crate) async fn resolve_lock_core(
    source: &dyn PartSource,
    lock: &LockFile,
    scope: &SourceScope,
) -> Result<Resolved, ResolveError> {
    let started = Instant::now();
    let mut reader = Reader::new(source, scope, &[], &[]);
    reader.load_withdrawn().await?;
    let mut held = Vec::with_capacity(lock.artefacts.len());
    for entry in &lock.artefacts {
        let unavailable = || ResolveError::LockPartUnavailable {
            part: entry.id.clone(),
        };
        let found = reader
            .lookup(Some(entry.kind), &entry.id, Some(&entry.version))
            .await?
            .ok_or_else(unavailable)?;
        let withdrawn = reader.admit(found.clone()).await.is_err();
        if withdrawn || prefixed_digest(&found.digest) != entry.content_hash {
            return Err(unavailable());
        }
        held.push(found);
    }
    let root = held
        .iter()
        .find(|h| {
            h.kind() == lock.composition.kind
                && h.id() == lock.composition.id
                && h.version() == lock.composition.version
        })
        .cloned()
        .ok_or_else(|| ResolveError::LockPartUnavailable {
            part: lock.composition.id.clone(),
        })?;
    let warnings = lifecycle_warnings(&root, &held);
    let practice_packs = lock
        .selections
        .practice_packs
        .iter()
        .filter_map(|selected| {
            lock.artefacts
                .iter()
                .find(|a| a.kind == PartKind::PracticePack && a.id == selected.id)
                .cloned()
        })
        .collect();
    let mut plan = ResolutionPlan {
        plan_hash: String::new(),
        root: lock.composition.clone(),
        target_harness: lock.target_harness.clone(),
        overlay_order: lock.overlays_applied.clone(),
        practice_packs,
        resolved: lock.artefacts.clone(),
        selections: lock.selections.clone(),
        warnings,
        withdrawn_snapshot: withdrawn_snapshot(reader.withdrawn_entries()),
        tool_policy: None,
    };
    let computed = compute_plan_hash(&plan);
    if computed != lock.plan_hash {
        return Err(ResolveError::LockMismatch {
            expected: lock.plan_hash.clone(),
            found: computed,
        });
    }
    plan.plan_hash = computed;
    plan.tool_policy = Some(compose_tool_policy(&root, &held)?);
    log::resolved(&plan, started.elapsed());
    Ok(Resolved { plan, held, root })
}

/// The warnings step 11 would give the same parts, without its refusals.
fn lifecycle_warnings(root: &Held, held: &[Held]) -> Vec<String> {
    held.iter()
        .filter(|h| h.key() != root.key() && h.status() == PartStatus::Deprecated)
        .map(|h| deprecation_warning(h.id()))
        .collect()
}

/// Steps 22c and 22d: the always-on instructions as one Markdown text.
pub(crate) async fn instructions_of(
    source: &dyn PartSource,
    resolved: &Resolved,
) -> Result<CompiledInstructions, ResolveError> {
    let mut ordered: Vec<&Held> = vec![&resolved.root];
    ordered.extend(
        resolved
            .held
            .iter()
            .filter(|h| h.key() != resolved.root.key()),
    );
    let mut pieces: Vec<String> = Vec::new();
    for part in ordered {
        if part.loading() != LoadingMode::AlwaysOn {
            continue;
        }
        let files = files_of(source, part).await?;
        for (name, bytes) in &files.0 {
            if let Some(text) = markdown_body(name, bytes) {
                let trimmed = trim_blank_lines(&text);
                if !trimmed.is_empty() {
                    pieces.push(trimmed);
                }
            }
        }
    }
    let mut agents_md = pieces.join("\n\n");
    if !agents_md.is_empty() {
        agents_md.push('\n');
    }
    log::instructions_compiled(&resolved.plan.plan_hash, pieces.len(), agents_md.len());
    Ok(CompiledInstructions {
        agents_md,
        lock: lock_of(&resolved.plan),
    })
}

/// LF line endings, without the blank lines before the first and after the last line of text.
fn trim_blank_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(start), Some(end)) => lines[start..=end].join("\n"),
        _ => String::new(),
    }
}

/// The policy a bundle manifest carries for a plan.
pub(crate) fn policy_of(plan: &ResolutionPlan) -> ResolvedToolPolicy {
    plan.tool_policy.clone().unwrap_or_default()
}

/// The identity of the loading default, for callers that list a part's loading.
pub(crate) fn loading_of(part: &PartRef) -> LoadingMode {
    part.loading.unwrap_or(kind_info(part.kind).default_loading)
}
