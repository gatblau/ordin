//! Installed agents follow the harness in use (PackageCatalogue.md, amendment 2026-02-05,
//! PKG-HAR-1 to PKG-HAR-4).

use ordin_types::domain::agents::AgentInstanceStatus;
use ordin_types::domain::HarnessKind;

use crate::catalogue::PackageCatalogue;
use crate::error::PackageError;
use crate::request::{resolve_request, PlanInputs};
use crate::store::{self, ChoiceSource, InstanceRow};

/// What `follow_harness` did, instance by instance (PKG-HAR-1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HarnessFollowed {
    pub followed: usize,
    pub failed: usize,
    pub unchanged: usize,
}

impl PackageCatalogue {
    /// PKG-HAR-1: installs again, for `harness.default_harness()`, every instance that is not
    /// `removed` and whose stored harness differs. Each instance's outcome is its own (PKG-HAR-3).
    // spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-1
    pub async fn follow_harness(&self) -> HarnessFollowed {
        let harness = self.inner.harness.default_harness();
        let rows = {
            let listed = match self.conn().await {
                Ok(mut conn) => store::live_instances(&mut conn).await,
                Err(error) => Err(error),
            };
            match listed {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::warn!(
                        component = "package_catalogue",
                        code = error.code,
                        "agent.harness_follow_failed"
                    );
                    return HarnessFollowed::default();
                }
            }
        };
        let mut out = HarnessFollowed::default();
        for row in rows {
            if row.harness == harness {
                out.unchanged += 1;
                continue;
            }
            let id = row.id;
            match self.follow_one(row.id, harness).await {
                Ok(()) => {
                    out.followed += 1;
                    tracing::info!(
                        component = "package_catalogue",
                        agent_instance_id = id.to_string().as_str(),
                        harness = harness_name(harness),
                        "agent.harness_followed"
                    );
                }
                // PKG-HAR-4: an instance with a running session is skipped, with a reason, no code.
                Err(None) => {
                    out.failed += 1;
                    tracing::warn!(
                        component = "package_catalogue",
                        agent_instance_id = id.to_string().as_str(),
                        reason = "running",
                        "agent.harness_follow_failed"
                    );
                }
                // PKG-HAR-3: the row, files and harness stay as they were; the next goes on.
                Err(Some(error)) => {
                    out.failed += 1;
                    tracing::warn!(
                        component = "package_catalogue",
                        agent_instance_id = id.to_string().as_str(),
                        code = error.code,
                        "agent.harness_follow_failed"
                    );
                }
            }
        }
        out
    }

    /// PKG-HAR-2: one instance, under its lock. `Err(None)` = skipped because a session runs.
    // spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-2
    async fn follow_one(
        &self,
        id: ordin_types::AgentInstanceId,
        harness: HarnessKind,
    ) -> Result<(), Option<PackageError>> {
        let lock = self.lock_for(id);
        let _held = lock.lock().await;
        // spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-4
        if self.inner.activity.running(id) {
            return Err(None);
        }
        let mut row: InstanceRow = self.live_row(id).await.map_err(Some)?;
        if row.status != AgentInstanceStatus::Ready {
            return Err(Some(PackageError::instance_not_ready(
                "the instance is not ready",
            )));
        }
        let scope = self.scope_of(&row).await.map_err(Some)?;
        let choices = {
            let mut conn = self.conn().await.map_err(Some)?;
            store::choices_of(&mut conn, id).await.map_err(Some)?
        };
        let inputs = PlanInputs {
            languages: row.workspace_languages.clone(),
            locale: row.interface_locale.clone(),
            harness,
            choices,
        };
        let request = resolve_request(
            scope,
            &row.agent_identifier,
            Some(row.package_version.clone()),
            &inputs,
        );
        let previous = row.harness;
        row.harness = harness;
        // PKG-HAR-3: `reinstall` saves the row only after the install and inspection succeed.
        self.reinstall(&mut row, &request, ChoiceSource::PersonChange)
            .await
            .map_err(|error| {
                row.harness = previous;
                Some(error)
            })
    }
}

fn harness_name(harness: HarnessKind) -> &'static str {
    match harness {
        HarnessKind::ClaudeCode => "claude_code",
        HarnessKind::CopilotCli => "copilot_cli",
    }
}
