//! The vocabulary set of one repository (AgentModel.md *Vocabulary set*, *Internal Logic* 11).

use std::collections::BTreeMap;

use ordin_types::domain::agents::{PartKind, VocabularyAxis};

use crate::error::AmError;
use crate::readers::VocabularyTermBody;
use crate::tree::ModelPart;

/// The eight axes, in declaration order.
const AXES: [VocabularyAxis; 8] = [
    VocabularyAxis::PackClass,
    VocabularyAxis::Category,
    VocabularyAxis::AudienceRole,
    VocabularyAxis::RuleSubvariant,
    VocabularyAxis::ArtefactStatus,
    VocabularyAxis::MemoryContentClass,
    VocabularyAxis::LoopStrategy,
    VocabularyAxis::RuntimeCapability,
];

/// The status terms ordin's code gives meaning to, present on `artefact-status` in every set
/// (ADR-091 item 4).
const STATUS_TERMS: [&str; 3] = ["draft", "approved", "deprecated"];

/// The kebab-case spelling of an axis (`pack-class`), as `VocabularyAxis`'s serde form writes it.
pub(crate) fn axis_str(axis: VocabularyAxis) -> &'static str {
    match axis {
        VocabularyAxis::PackClass => "pack-class",
        VocabularyAxis::Category => "category",
        VocabularyAxis::AudienceRole => "audience-role",
        VocabularyAxis::RuleSubvariant => "rule-subvariant",
        VocabularyAxis::ArtefactStatus => "artefact-status",
        VocabularyAxis::MemoryContentClass => "memory-content-class",
        VocabularyAxis::LoopStrategy => "loop-strategy",
        VocabularyAxis::RuntimeCapability => "runtime-capability",
    }
}

pub(crate) fn axis_from_str(text: &str) -> Option<VocabularyAxis> {
    AXES.into_iter().find(|axis| axis_str(*axis) == text)
}

/// Position of an axis in declaration order; `VocabularyAxis` is not `Ord`, so the set is keyed by it.
fn axis_index(axis: VocabularyAxis) -> u8 {
    axis as u8
}

/// The terms of every `vocabulary` part read in one repository (and the repositories it borrows
/// from), one per axis.
#[derive(Debug, Clone, Default)]
pub struct VocabularySet {
    axes: BTreeMap<u8, BTreeMap<String, VocabularyTermBody>>,
}

impl VocabularySet {
    /// Refuses a second vocabulary part for one axis (AM_VOCABULARY_AXIS_DUPLICATE).
    pub fn from_parts(parts: &[&ModelPart]) -> Result<VocabularySet, AmError> {
        let mut set = VocabularySet::default();
        for part in parts.iter().filter(|p| p.kind == PartKind::Vocabulary) {
            let Some(body) = part.manifest.vocabulary()? else {
                continue;
            };
            let key = axis_index(body.axis);
            if set.axes.contains_key(&key) {
                return Err(AmError::VocabularyAxisDuplicate { axis: body.axis });
            }
            let terms = body
                .terms
                .into_iter()
                .map(|term| (term.code.clone(), term))
                .collect();
            set.axes.insert(key, terms);
        }
        Ok(set)
    }

    pub fn has(&self, axis: VocabularyAxis, code: &str) -> bool {
        self.axes
            .get(&axis_index(axis))
            .is_some_and(|terms| terms.contains_key(code))
            || (axis == VocabularyAxis::ArtefactStatus && STATUS_TERMS.contains(&code))
    }

    /// The codes of one axis, sorted.
    pub fn codes(&self, axis: VocabularyAxis) -> Vec<String> {
        let mut codes: Vec<String> = self
            .axes
            .get(&axis_index(axis))
            .map(|terms| terms.keys().cloned().collect())
            .unwrap_or_default();
        if axis == VocabularyAxis::ArtefactStatus {
            for status in STATUS_TERMS {
                if !codes.iter().any(|c| c == status) {
                    codes.push(status.to_owned());
                }
            }
            codes.sort();
        }
        codes
    }

    /// The label a vocabulary part gives the term; None when no part defines it.
    pub fn label(&self, axis: VocabularyAxis, code: &str) -> Option<String> {
        self.axes
            .get(&axis_index(axis))
            .and_then(|terms| terms.get(code))
            .map(|term| term.label.clone())
    }

    /// `rule-subvariant` terms resolve to `Rule`.
    // SPEC-GAP-10: the spec names authored kinds `rule-layer` and `rule-orchestrator` resolving through
    // the `rule-subvariant` vocabulary without saying whether its codes carry the `rule-` prefix. Both
    // spellings resolve: the authored value as a code, or the authored value without `rule-`.
    pub fn rule_subvariant(&self, authored: &str) -> Option<PartKind> {
        let axis = VocabularyAxis::RuleSubvariant;
        let found = self.has(axis, authored)
            || authored
                .strip_prefix("rule-")
                .is_some_and(|code| self.has(axis, code));
        found.then_some(PartKind::Rule)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_spellings_match_serde() {
        for axis in AXES {
            assert_eq!(
                serde_json::to_value(axis).unwrap(),
                serde_json::Value::String(axis_str(axis).to_owned())
            );
            assert_eq!(axis_from_str(axis_str(axis)), Some(axis));
        }
    }

    #[test]
    fn status_terms_always_present() {
        let set = VocabularySet::default();
        assert!(set.has(VocabularyAxis::ArtefactStatus, "draft"));
        assert!(!set.has(VocabularyAxis::Category, "draft"));
        assert_eq!(
            set.codes(VocabularyAxis::ArtefactStatus),
            vec!["approved", "deprecated", "draft"]
        );
        assert_eq!(set.label(VocabularyAxis::ArtefactStatus, "draft"), None);
        assert_eq!(set.rule_subvariant("rule-layer"), None);
    }
}
