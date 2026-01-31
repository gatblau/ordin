//! Settings › Memory's behaviour settings (MEM-SET-1 to MEM-SET-5; amended January 2026; user
//! decision; Approved by the user January 2026): held here, enforced on every write and recall.

use chrono::{DateTime, Utc};
use memory::types::domain::{Fact, MemoryClass};

use crate::error::MemoryError;
use crate::model::{KeepMode, MemoryCaller, MemorySettings};
use crate::remember::{RowSeed, AGENT_UNSOURCED_TRUST};
use crate::service::{Inner, MemoryService};

/// The longest "suggest removing after" period, in days.
const DAYS_MAX: u16 = 3650;

impl MemoryService {
    /// The settings in force.
    pub fn settings(&self) -> MemorySettings {
        self.inner.settings()
    }

    /// Replaces the settings; the next write and recall follow them. A period outside 1–3,650 days
    /// is refused with `MEM_LIMIT_INVALID` and nothing changes.
    // spec: ordin-desktop/components/MemoryService#MEM-SET-1
    pub fn set_settings(&self, settings: MemorySettings) -> Result<(), MemoryError> {
        if let Some(days) = settings.suggest_removal_after_days {
            if !(1..=DAYS_MAX).contains(&days) {
                return Err(MemoryError::invalid(
                    "MEM_LIMIT_INVALID",
                    "suggest_removal_after_days",
                ));
            }
        }
        if let Ok(mut slot) = self.inner.settings.write() {
            *slot = settings;
        }
        tracing::info!(
            target: "ordin::memory",
            keep = match settings.keep {
                KeepMode::OnItsOwn => "on_its_own",
                KeepMode::AskFirst => "ask_first",
            },
            conversations = settings.learn_from_conversations,
            documents = settings.learn_from_documents,
            removal_days = settings.suggest_removal_after_days.unwrap_or(0),
            everywhere = settings.use_everywhere,
            say_which = settings.say_which_facts,
            "memory.settings_changed"
        );
        Ok(())
    }
}

impl Inner {
    pub(crate) fn settings(&self) -> MemorySettings {
        self.settings.read().map(|s| *s).unwrap_or_default()
    }

    /// MEM-SET-2 and MEM-SET-3 for a write by Ordin, a crew member, an agent or a hosted agent:
    /// a source the person turned off refuses the write (`MEM_SOURCE_TURNED_OFF`); with "ask first",
    /// a fact Ordin takes from a conversation is held for the person. A summary of kept facts, a
    /// person's write and a knowledge path are not affected.
    // spec: ordin-desktop/components/MemoryService#MEM-SET-2
    // spec: ordin-desktop/components/MemoryService#MEM-SET-3
    pub(crate) fn learning_gate(
        &self,
        who: &MemoryCaller,
        seed: &mut RowSeed,
        summary: bool,
    ) -> Result<(), MemoryError> {
        let learner = matches!(
            who,
            MemoryCaller::Ordin { .. }
                | MemoryCaller::CrewMember { .. }
                | MemoryCaller::Agent { .. }
                | MemoryCaller::HostedAgent { .. }
        );
        if !learner || summary {
            return Ok(());
        }
        let s = self.settings();
        let from_document = seed.origin_kind == "document";
        if (from_document && !s.learn_from_documents)
            || (!from_document && !s.learn_from_conversations)
        {
            return Err(MemoryError::Refused {
                code: "MEM_SOURCE_TURNED_OFF",
            });
        }
        if s.keep == KeepMode::AskFirst
            && matches!(who, MemoryCaller::Ordin { .. })
            && !from_document
            && seed.gate_trust.is_none()
        {
            seed.gate_trust = Some(AGENT_UNSOURCED_TRUST);
        }
        Ok(())
    }

    /// MEM-SET-4: whether a fact is suggested for removal. `decided` is the decay rule's answer
    /// (today's behaviour), used when no period is set.
    // spec: ordin-desktop/components/MemoryService#MEM-SET-4
    pub(crate) fn fading_of(&self, fact: &Fact, now: DateTime<Utc>, decided: bool) -> bool {
        match self.settings().suggest_removal_after_days {
            None => decided,
            Some(days) => unused_for(fact, now, days),
        }
    }
}

/// An episodic fact, true now, not used (recalled, else written) for at least `days` days.
pub(crate) fn unused_for(fact: &Fact, now: DateTime<Utc>, days: u16) -> bool {
    fact.memory_class == MemoryClass::Episodic
        && fact.valid_to.is_none()
        && now - memory::maintenance::fading_since(fact) >= chrono::Duration::days(i64::from(days))
}
