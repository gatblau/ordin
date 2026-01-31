//! Originals in repositories (ADR-069, A-36; Internal Logic 4).

use std::path::PathBuf;
use std::time::Instant;

use ordin_types::time::Timestamp;
use ordin_types::PlaceId;
use tokio::sync::broadcast;

use crate::error::ProjectError;
use crate::git::{self, ChangeCommit};
use crate::service::{lock, ProjectService};
use crate::store;
use crate::types::{ProjectFetchOutcome, RemoteBranchState, RepositoryOriginChange};

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl ProjectService {
    /// Internal Logic 4.1.
    pub async fn follow_original(
        &self,
        place_id: PlaceId,
        path: &str,
        branch: &str,
    ) -> Result<(), ProjectError> {
        let others = store::followed_count_of_places(&self.pool, place_id).await?;
        if others >= i64::from(crate::MAX_FOLLOWED_REPOSITORIES) {
            return Err(ProjectError::TooManyFollowed);
        }
        store::follow(&self.pool, place_id, path, branch).await
    }

    pub async fn unfollow_original(
        &self,
        place_id: PlaceId,
        path: &str,
        branch: &str,
    ) -> Result<(), ProjectError> {
        store::unfollow(&self.pool, place_id, path, branch).await
    }

    pub fn subscribe_original_changes(&self) -> broadcast::Receiver<RepositoryOriginChange> {
        self.originals.subscribe()
    }

    /// Fetches the branch and records the outcome; returns the new tip.
    async fn fetch_tip(
        &self,
        place: PlaceId,
        folder: PathBuf,
        branch: &str,
    ) -> Result<String, ProjectError> {
        let started = Instant::now();
        let result = self.fetch_branch_now(folder, branch).await;
        let (result_word, at) = (
            if result.is_ok() { "fetched" } else { "failed" },
            Timestamp::now(),
        );
        crate::log::git_fetch(place, branch, result_word, elapsed_ms(started));
        if let Err(error) = &result {
            let limited = matches!(error, ProjectError::RateLimited);
            store::fetch_failed(&self.pool, place, branch, error.code(), at, limited).await?;
        }
        result
    }

    async fn fetch_branch_now(
        &self,
        folder: PathBuf,
        branch: &str,
    ) -> Result<String, ProjectError> {
        let probe = folder.clone();
        git::blocking(move || git::open(&probe).map(|_| ())).await?;
        let auth = self.remote_auth(&folder).await?;
        let limit = lock(&self.tunables).network_timeout;
        let target = branch.to_owned();
        git::network(limit, move |stop| {
            git::fetch_branch(&folder, &target, &auth, stop)
        })
        .await
    }

    /// Internal Logic 4.3 and 4.4.
    // spec: ordin-desktop/components/ProjectService#ADR-069
    // spec: ordin-desktop/components/ProjectService#A-36
    pub async fn fetch_followed(
        &self,
        repo_place: PlaceId,
        branch: &str,
    ) -> Result<ProjectFetchOutcome, ProjectError> {
        let folder = self.place_folder(repo_place).await?;
        let previous = store::remote_commit(&self.pool, repo_place, branch).await?;
        let tip = self.fetch_tip(repo_place, folder.clone(), branch).await?;
        let checked_at = Timestamp::now();
        let mut changes = Vec::new();
        for followed in store::followed_paths(&self.pool, repo_place, branch).await? {
            let (probe, file, new_tip, old_tip, old_blob) = (
                folder.clone(),
                followed.path.clone(),
                tip.clone(),
                previous.clone(),
                followed.last_blob.clone(),
            );
            let first_fetch = old_blob.is_none();
            let found = git::blocking(move || {
                git::path_change(
                    &probe,
                    &file,
                    &new_tip,
                    old_tip.as_deref(),
                    old_blob.as_deref(),
                    first_fetch,
                )
            })
            .await?;
            let commit = found.changed.as_ref().map(|c| c.commit.clone());
            store::set_followed_blob(
                &self.pool,
                repo_place,
                &followed.path,
                branch,
                found.blob.as_deref(),
                commit.as_deref(),
            )
            .await?;
            if let Some(ChangeCommit {
                commit,
                author,
                committed_at,
            }) = found.changed
            {
                crate::log::original_changed(repo_place, &commit);
                let change = RepositoryOriginChange {
                    place_id: repo_place,
                    path: followed.path,
                    branch: branch.to_owned(),
                    commit,
                    author,
                    committed_at,
                };
                // No subscriber yet is not a failure: the change is in the outcome.
                let _ = self.originals.send(change.clone());
                changes.push(change);
            }
        }
        store::fetch_succeeded(&self.pool, repo_place, branch, &tip, checked_at).await?;
        Ok(ProjectFetchOutcome {
            place_id: repo_place,
            branch: branch.to_owned(),
            commit: tip,
            checked_at,
            changes,
        })
    }

    /// The program calls it when a workspace opens, with that workspace's repository places.
    pub async fn workspace_opened(&self, repo_places: &[PlaceId]) {
        let followed = match store::followed_branches(&self.pool).await {
            Ok(followed) => followed,
            Err(error) => {
                crate::log::background_failed("workspace_opened", error.code());
                return;
            }
        };
        for row in followed {
            if repo_places.contains(&row.place_id) {
                if let Err(error) = self.fetch_followed(row.place_id, &row.branch).await {
                    crate::log::background_failed("fetch_followed", error.code());
                }
            }
        }
    }

    /// One pass of the timer: every followed branch at start, otherwise those whose interval passed.
    pub(crate) async fn fetch_due(&self, every: bool) {
        let followed = match store::followed_branches(&self.pool).await {
            Ok(followed) => followed,
            Err(error) => {
                crate::log::background_failed("fetch_due", error.code());
                return;
            }
        };
        let now = Timestamp::now().unix_millis();
        for row in followed {
            let due = every
                || row
                    .last_checked_at
                    .is_none_or(|at| now - at.unix_millis() >= row.interval_s * 1000);
            if due {
                if let Err(error) = self.fetch_followed(row.place_id, &row.branch).await {
                    crate::log::background_failed("fetch_followed", error.code());
                }
            }
        }
    }

    /// Internal Logic 4.5: from the object store, never the working copy.
    // spec: ordin-desktop/components/ProjectService#ADR-069
    pub async fn read_at_commit(
        &self,
        repo_place: PlaceId,
        commit: &str,
        path: &str,
    ) -> Result<Vec<u8>, ProjectError> {
        let folder = self.place_folder(repo_place).await?;
        let (commit, path) = (commit.to_owned(), path.to_owned());
        git::blocking(move || git::read_at_commit(&folder, &commit, &path)).await
    }

    /// Internal Logic 4.6.
    pub async fn remote_state(
        &self,
        place_id: PlaceId,
        branch: &str,
    ) -> Result<RemoteBranchState, ProjectError> {
        let folder = self.place_folder(place_id).await?;
        let commit = self.fetch_tip(place_id, folder, branch).await?;
        let checked_at = Timestamp::now();
        Ok(RemoteBranchState {
            branch: branch.to_owned(),
            commit,
            checked_at,
        })
    }

    /// Internal Logic 4.6: the remote `HEAD` symbolic ref, else `main`.
    pub async fn default_branch(&self, place_id: PlaceId) -> Result<String, ProjectError> {
        let folder = self.place_folder(place_id).await?;
        let probe = folder.clone();
        git::blocking(move || git::open(&probe).map(|_| ())).await?;
        let auth = self.remote_auth(&folder).await?;
        let limit = lock(&self.tunables).network_timeout;
        let name = git::network(limit, move |stop| {
            git::remote_default_branch(&folder, &auth, stop)
        })
        .await?;
        Ok(name.unwrap_or_else(|| "main".to_owned()))
    }
}
