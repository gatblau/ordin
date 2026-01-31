//! The starter agents (PackageCatalogue.md, amendment January 2026, PKG-DOC-1 to PKG-DOC-3): agents
//! from the bundled catalogue added to every workspace once, as ordinary instances the person can
//! give work to, update and remove. The program calls `add_starter_agents` at start for every active
//! workspace and after a workspace is created (OrdinDesktop.md DSK-DOC-1).

use ordin_catalogue::PartStateLabel;
use ordin_resolver::model::{PartKind, BUNDLED_REPOSITORY_ID};
use ordin_types::{AgentInstanceId, WorkspaceId};

use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::store;
use crate::types::{AddAgentInput, AddOutcome};

/// WorkspaceProfile identifiers in the bundled catalogue, added to each workspace once, in order.
// spec: ordin-desktop/components/PackageCatalogue#PKG-DOC-1
pub const STARTER_AGENTS: &[&str] = &["urn:ordin:default:workspaces:document-agent"];

/// What one starter agent's add came to at step 3, before the offer is recorded.
enum Offer {
    /// Added, or found already there, or refused for good: the offer row is written.
    Recorded,
    /// Not in the bundled catalogue: no row, so a later version that ships it offers it.
    Missing,
}

impl PackageCatalogue {
    /// Adds each starter agent to the workspace once. Called by the program at start for every active
    /// workspace, and after a workspace is created (OrdinDesktop.md DSK-DOC-1).
    // spec: ordin-desktop/components/PackageCatalogue#PKG-DOC-2
    pub async fn add_starter_agents(
        &self,
        workspace_id: WorkspaceId,
        workspace_languages: Vec<String>,
        interface_locale: &str,
    ) -> Result<Vec<AgentInstanceId>, PackageError> {
        let mut added = Vec::new();
        for &identifier in STARTER_AGENTS {
            // Step 1: offered once; a person who removed it keeps it removed (PKG-DOC-3).
            {
                let mut conn = self.conn().await?;
                if store::starter_offered(&mut conn, workspace_id, identifier).await? {
                    continue;
                }
            }
            let offer = self
                .offer_starter(
                    workspace_id,
                    identifier,
                    &workspace_languages,
                    interface_locale,
                    &mut added,
                )
                .await?;
            if let Offer::Recorded = offer {
                let mut conn = self.conn().await?;
                store::record_starter_offer(&mut conn, workspace_id, identifier).await?;
            }
        }
        Ok(added)
    }

    /// PKG-DOC-2 steps 2 and 3 for one identifier. An `Err` is returned only when the call must be
    /// tried again later (nothing is recorded for this or later identifiers).
    async fn offer_starter(
        &self,
        workspace_id: WorkspaceId,
        identifier: &str,
        workspace_languages: &[String],
        interface_locale: &str,
        added: &mut Vec<AgentInstanceId>,
    ) -> Result<Offer, PackageError> {
        let missing = || {
            tracing::warn!(
                component = "package_catalogue",
                identifier,
                "agent.starter_missing"
            );
            Ok(Offer::Missing)
        };
        // Step 2.
        let view = match self
            .inner
            .ports
            .catalogue
            .find_part(
                BUNDLED_REPOSITORY_ID,
                PartKind::WorkspaceProfile,
                identifier,
                None,
            )
            .await
        {
            Ok(view) => view,
            Err(error) if matches!(error.code, "CAT_PART_UNKNOWN" | "CAT_REPOSITORY_UNKNOWN") => {
                return missing();
            }
            Err(error) => {
                return Err(PackageError::library_unavailable(
                    error.retryable,
                    error.code,
                ))
            }
        };
        if view.state_label != PartStateLabel::Current {
            return missing();
        }
        // Step 3.
        let input = AddAgentInput {
            workspace_id,
            agent: view.part.id,
            workspace_languages: workspace_languages.to_vec(),
            interface_locale: interface_locale.to_owned(),
            choices: Vec::new(),
        };
        let not_added = |outcome: &str| {
            tracing::warn!(
                component = "package_catalogue",
                identifier,
                workspace_id = %workspace_id,
                outcome,
                "agent.starter_not_added"
            );
        };
        match self.add(input).await {
            // `add` journals the add and raises the catalogue's notice (DSK-DOC-1, OH-DIR-1).
            Ok(AddOutcome::Ready { agent }) => added.push(agent.instance.id),
            Ok(AddOutcome::Unsafe) => not_added("unsafe"),
            Ok(AddOutcome::NeedsItApproval { .. }) => not_added("needs_it_approval"),
            Ok(AddOutcome::PackMissing { .. }) => not_added("pack_missing"),
            // The person added it first.
            Err(error) if error.code == "PKG_ALREADY_ADDED" => {}
            Err(error) if error.retryable => return Err(error),
            Err(error) => not_added(error.code),
        }
        Ok(Offer::Recorded)
    }
}
