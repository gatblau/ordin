//! The entry point: the four plug-ins in detection priority order (FormatPlugins.md *Public
//! Interface*, *Internal Logic* 2, 11).

use std::time::Instant;

use crate::apm::ApmPlugin;
use crate::claude::ClaudeCodePlugin;
use crate::copilot::CopilotPlugin;
use crate::error::FormatError;
use crate::guard::guard;
use crate::log;
use crate::ordin::OrdinPlugin;
use crate::propose::propose;
use crate::types::{
    Detection, FormatId, FormatPlugin, KindProposal, ReadOutcome, ReadRequest, SourceTree,
};

/// The four plug-ins of the first release, in detection priority order. Compiled in; nothing is
/// loaded at run time (ADR-087).
// spec: ordin-desktop/components/FormatPlugins#SA-RECON-01
pub struct FormatPlugins {
    plugins: [Box<dyn FormatPlugin>; 4],
}

impl FormatPlugins {
    pub fn builtin() -> FormatPlugins {
        FormatPlugins {
            plugins: [
                Box::new(OrdinPlugin),
                Box::new(ApmPlugin),
                Box::new(ClaudeCodePlugin),
                Box::new(CopilotPlugin),
            ],
        }
    }

    pub fn get(&self, id: FormatId) -> &dyn FormatPlugin {
        let [ordin, apm, claude, copilot] = &self.plugins;
        match id {
            FormatId::Ordin => ordin.as_ref(),
            FormatId::Apm => apm.as_ref(),
            FormatId::ClaudeCode => claude.as_ref(),
            FormatId::Copilot => copilot.as_ref(),
        }
    }

    /// Every plug-in that recognises the tree, in priority order Ordin, Apm, ClaudeCode, Copilot.
    pub fn detect(&self, tree: &SourceTree) -> Result<Vec<Detection>, FormatError> {
        guard(tree)?;
        let found: Vec<Detection> = self.plugins.iter().filter_map(|p| p.detect(tree)).collect();
        log::detected(&found);
        if found.is_empty() {
            Err(FormatError::NotRecognised)
        } else {
            Ok(found)
        }
    }

    /// Detect, then read with the first match.
    pub fn read_any(
        &self,
        tree: &SourceTree,
        request: &ReadRequest,
    ) -> Result<(Detection, ReadOutcome), FormatError> {
        let started = Instant::now();
        let mut detected: Option<Detection> = None;
        let result = self.detect(tree).and_then(|found| {
            let first = found.into_iter().next().ok_or(FormatError::NotRecognised)?;
            detected = Some(first.clone());
            let outcome = self.get(first.format).read(tree, request)?;
            Ok((first, outcome))
        });
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &result {
            Ok((detection, outcome)) => log::read_ok(
                detection,
                outcome.parts.len(),
                outcome.mapping_report.items.len(),
                elapsed,
            ),
            Err(error) => log::read_failed(detected.as_ref(), error, elapsed),
        }
        result
    }

    /// Proposes the kind of one file for "Import as a part...".
    pub fn propose_kind(&self, path: &str, bytes: &[u8]) -> Result<KindProposal, FormatError> {
        propose(path, bytes)
    }
}
