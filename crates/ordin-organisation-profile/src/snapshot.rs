//! What the service serves: the snapshot in force, the outcome of a refresh and the status line
//! (OrganisationProfile.md *Public Interface*, *The service*).

use ordin_types::time::Timestamp;
use ordin_types::OrganisationProfileId;
use serde::{Deserialize, Serialize};

use crate::document::{HarnessName, OrganisationProfileDocument};

/// The settings in force: the active verified profile, or the documented defaults.
// spec: ordin-desktop/components/OrganisationProfile#SA-RECON-02
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSnapshot {
    pub document: OrganisationProfileDocument,
    pub source: SnapshotSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotSource {
    Verified {
        profile_id: OrganisationProfileId,
        profile_version: String,
        activated_at: Timestamp,
    },
    Defaults,
}

impl ProfileSnapshot {
    /// Every section at the default 2D-2 gives it.
    pub fn defaults() -> ProfileSnapshot {
        ProfileSnapshot {
            document: OrganisationProfileDocument::defaults(),
            source: SnapshotSource::Defaults,
        }
    }

    /// The harness a new agent instance uses when nothing else names one: the first entry of
    /// `document.harnesses` (2D-2 default `[claude_code]`, so never empty after validation).
    /// The program maps `HarnessName::ClaudeCode` → `HarnessKind::ClaudeCode` and
    /// `HarnessName::Copilot` → `HarnessKind::CopilotCli` for the Package Catalogue's
    /// `HarnessSettings`.
    pub fn default_harness(&self) -> HarnessName {
        // Validation keeps 1–2 entries; an empty list (never served) falls back to the 2D-2 default.
        self.document
            .harnesses
            .first()
            .map_or(HarnessName::ClaudeCode, |h| h.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    /// A newer verified profile is now active.
    Activated { profile_version: String },
    /// The address served the active profile again (HTTP 304, or the same digest).
    Unchanged,
    /// The fetched profile failed a check; the last verified one is kept.
    Refused { code: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileStatus {
    pub state: ProfileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<ProfileFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileState {
    /// Agent profile.
    NotUsed,
    /// No profile address is known (desktop: R-11, setup cannot finish; server: A-40 defaults).
    NoAddress,
    /// An address is known and no verified profile has been activated yet.
    NeverFetched,
    /// A verified profile is active and the last check succeeded within 3 hours.
    Active,
    /// A verified profile is active; no check has succeeded for 3 hours.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileFailure {
    pub code: String,
    pub at: Timestamp,
}
