//! The closure walk, the slot ledger and conflict detection (ResolverAndCompiler.md *Internal Logic*
//! 7 and 9).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use ordin_agent_model::{relation_kind, ManifestRef, PartKind, PartRef};

use crate::conflict::{Conflict, ConflictCandidate};
use crate::error::ResolveError;
use crate::held::{Held, NodeKey, Reader};

/// Closure limit: 1,000 parts (*assumed*: five times the 200-part case shown).
pub(crate) const MAX_CLOSURE_PARTS: usize = 1_000;
/// Reference depth limit (*assumed*).
pub(crate) const MAX_DEPTH: usize = 64;

/// What a slot's answer is: (kind, name, version, digest).
type Identity = (PartKind, String, String, String);

#[derive(Debug, Clone)]
pub(crate) struct Contribution {
    pub held: Held,
    /// The seeds whose closure reaches the part.
    pub requesters: BTreeSet<PartRef>,
}

/// Every (seed, resolved part) contribution, keyed by slot `"<Kind>/<name>"`.
#[derive(Debug, Default)]
pub(crate) struct Closure {
    by_slot: BTreeMap<String, BTreeMap<Identity, Contribution>>,
    distinct: HashSet<Identity>,
}

struct Frame {
    held: Held,
    refs: Vec<ManifestRef>,
    next: usize,
}

fn slot_of(held: &Held) -> String {
    format!("{:?}/{}", held.kind(), held.name())
}

fn identity_of(held: &Held) -> Identity {
    (
        held.kind(),
        held.name(),
        held.version().to_owned(),
        held.digest.0.clone(),
    )
}

impl Closure {
    /// Step 7 for one seed: a depth-first walk over `manifest.references()`. A node met again on the
    /// current path is a cycle; a node already finished for this seed is not walked twice.
    pub(crate) async fn walk(
        &mut self,
        reader: &mut Reader<'_>,
        seed: Held,
    ) -> Result<(), ResolveError> {
        let seed_ref = seed.part_ref();
        self.record(&seed, &seed_ref)?;
        let mut done: HashSet<NodeKey> = HashSet::new();
        let mut stack = vec![Frame {
            refs: seed.manifest().references(),
            held: seed,
            next: 0,
        }];
        while let Some(frame) = stack.last_mut() {
            if frame.next >= frame.refs.len() {
                let finished = stack.pop();
                if let Some(finished) = finished {
                    done.insert(finished.held.key());
                }
                continue;
            }
            let reference = frame.refs[frame.next].clone();
            frame.next += 1;
            let from = frame.held.id().to_owned();
            let child = fetch_reference(reader, &from, &reference).await?;
            let key = child.key();
            if stack.iter().any(|f| f.held.key() == key) || stack.len() > MAX_DEPTH {
                let mut path: Vec<String> = stack.iter().map(|f| f.held.id().to_owned()).collect();
                path.push(child.id().to_owned());
                return Err(ResolveError::DependencyCycle { path });
            }
            if done.contains(&key) {
                continue;
            }
            self.record(&child, &seed_ref)?;
            stack.push(Frame {
                refs: child.manifest().references(),
                held: child,
                next: 0,
            });
        }
        Ok(())
    }

    fn record(&mut self, held: &Held, seed: &PartRef) -> Result<(), ResolveError> {
        let identity = identity_of(held);
        self.distinct.insert(identity.clone());
        if self.distinct.len() > MAX_CLOSURE_PARTS {
            return Err(ResolveError::ClosureTooLarge {
                parts: u32::try_from(self.distinct.len()).unwrap_or(u32::MAX),
            });
        }
        let entry = self
            .by_slot
            .entry(slot_of(held))
            .or_default()
            .entry(identity)
            .or_insert_with(|| Contribution {
                held: held.clone(),
                requesters: BTreeSet::new(),
            });
        entry.requesters.insert(seed.clone());
        Ok(())
    }

    /// Step 9: a slot holding two or more distinct identities, sorted by slot; candidates sorted by
    /// identity, each with its requesting seeds sorted. No pin resolves a conflict.
    pub(crate) fn conflicts(&self) -> Vec<Conflict> {
        self.by_slot
            .iter()
            .filter(|(_, identities)| identities.len() > 1)
            .map(|(slot, identities)| Conflict {
                slot: slot.clone(),
                candidates: identities
                    .values()
                    .map(|contribution| ConflictCandidate {
                        part: contribution.held.part_ref(),
                        requested_by: contribution.requesters.iter().cloned().collect(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// The parts of a conflict-free closure, sorted by (kind, name, version).
    pub(crate) fn into_parts(self) -> Vec<Held> {
        let mut parts: Vec<Held> = self
            .by_slot
            .into_values()
            .filter_map(|identities| identities.into_values().next().map(|c| c.held))
            .collect();
        parts.sort_by(|a, b| {
            a.kind()
                .cmp(&b.kind())
                .then_with(|| a.name().cmp(&b.name()))
                .then_with(|| a.version().cmp(b.version()))
        });
        parts
    }
}

/// Resolves one reference: a known relation kind through `resolve`, an unknown one through
/// `resolve_any_kind`; a withdrawn part is replaced or refused.
async fn fetch_reference(
    reader: &mut Reader<'_>,
    from: &str,
    reference: &ManifestRef,
) -> Result<Held, ResolveError> {
    let kind = relation_kind(&reference.relation);
    let found = reader.lookup(kind, &reference.id, None).await?;
    let held = found.ok_or_else(|| ResolveError::ReferenceUnresolved {
        from: from.to_owned(),
        id: reference.id.clone(),
        relation: reference.relation.clone(),
    })?;
    reader.admit(held).await
}
