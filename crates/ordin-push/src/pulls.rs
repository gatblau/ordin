//! Pull requests (PushService.md Internal Logic F; ADR-084, ADR-091, ADR-092) and team knowledge
//! shares (amendment January 2026, Internal Logic I; ADR-103).

use std::path::PathBuf;

use ordin_approval::ApprovalRequest;
use ordin_journal::NewExternalAction;
use ordin_projects::{BranchWithFiles, GithubError, OpenPullRequest};
use ordin_types::domain::agents::PullRequestRef;
use ordin_types::domain::runs::{ApprovalState, ExternalActionKind};
use ordin_types::wire::approval::{ApprovalAsker, RejectStyle};
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{ApprovalId, CodedError, ExternalActionKey, TeamShareId};

use crate::error::PushError;
use crate::gitops::{self, Creds, GitFailure, GitStep};
use crate::model::{
    ApprovedPullRequest, CardText, PullRequestContent, PullRequestProposal, PushNotice,
    SubmissionId, SubmissionOrigin, TeamShareProposal,
};
use crate::service::{lock, PushService};
use crate::store::{self, SubmissionRow, TeamShareRow, TeamUpdate};
use crate::text;

/// The proposal as stored: without file bytes (Data Model).
fn stored_form(proposal: &PullRequestProposal) -> Result<String, PushError> {
    let mut copy = proposal.clone();
    if let PullRequestContent::Files { files } = &mut copy.content {
        files.clear();
    }
    serde_json::to_string(&copy).map_err(|_| PushError::store(false, "proposal does not serialise"))
}

fn valid_ref(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 255
        && git2::Reference::is_valid_name(&format!("refs/heads/{branch}"))
}

/// The 2C card limits this component must respect when it builds a request.
fn card_valid(card: &CardText) -> bool {
    let q = card.question.chars().count();
    let v = card.verb.chars().count();
    let w = card.where_line.chars().count();
    let c = card.changes.chars().count();
    (1..=120).contains(&q)
        && card.question.ends_with('?')
        && (1..=32).contains(&v)
        && !["ok", "yes", "continue"].contains(&card.verb.to_ascii_lowercase().as_str())
        && (1..=200).contains(&w)
        && (1..=50).contains(&card.affects.len())
        && (1..=1000).contains(&c)
}

fn request_from(
    workspace: ordin_types::WorkspaceId,
    card: &CardText,
    action: &str,
    destination: String,
) -> ApprovalRequest {
    ApprovalRequest {
        workspace_id: workspace,
        asked_by: ApprovalAsker::Person,
        question: card.question.clone(),
        verb: card.verb.clone(),
        where_line: card.where_line.clone(),
        affects: card.affects.clone(),
        changes: card.changes.clone(),
        content: card.content.clone(),
        reject_style: RejectStyle::Reject,
        show_details: card.content.is_some(),
        proposal: ActionProposal {
            action: action.to_owned(),
            destination,
            affects: card.affects.clone(),
            consequence: card.changes.clone(),
        },
        asker: None,
        leaves_company: false,
    }
}

/// F.3: the submission outcome for what GitHub answered.
fn github_code(error: &GithubError) -> &'static str {
    match error {
        GithubError::NotFound => "PUSH_NO_REVIEW_RIGHT",
        GithubError::SignInNeeded => "PUSH_SIGN_IN_NEEDED",
        GithubError::Unreachable | GithubError::RateLimited { .. } => "PUSH_ORIGIN_UNREACHABLE",
        GithubError::Conflict { .. } => "PUSH_BRANCH_MOVED",
        GithubError::HostNotSupported | GithubError::Other(_) => "PUSH_DESTINATION_REFUSED",
    }
}

impl PushService {
    // ---- F. Pull requests ----

    /// F.1: stores the submission and raises its card.
    pub async fn propose_pull_request(
        &self,
        req: PullRequestProposal,
    ) -> Result<SubmissionId, PushError> {
        let pool = self.pool()?;
        let title = req.title.chars().count();
        if !(1..=256).contains(&title)
            || req.body.chars().count() > 65_536
            || !valid_ref(&req.branch)
            || req.base.is_empty()
            || req.labels.len() > 10
            || req.host.is_empty()
            || req.repository.is_empty()
            || !card_valid(&req.card)
        {
            return Err(PushError::new(
                "PUSH_REQUEST_INVALID",
                "pull request proposal",
            ));
        }
        let id = SubmissionId::new();
        let origin =
            serde_json::to_string(&req.origin).map_err(|_| PushError::store(false, "origin"))?;
        store::insert_submission(pool, id, req.workspace_id, &origin, &stored_form(&req)?).await?;
        if let PullRequestContent::Files { files } = &req.content {
            if let Err(error) = store::put_pull_request_files(pool, id, files).await {
                let _ = store::delete_submission(pool, id).await;
                return Err(error);
            }
        }
        let request = request_from(
            req.workspace_id,
            &req.card,
            text::PR_ACTION,
            format!("{}/{}", req.host, req.repository),
        );
        match self.deps.approvals.request(request).await {
            Ok(card) => {
                store::update_submission(
                    pool,
                    id,
                    "proposed",
                    Some(card.approval_id),
                    None,
                    None,
                    None,
                )
                .await?;
            }
            Err(error) => {
                let _ = store::delete_pull_request_files(pool, id).await;
                let _ = store::delete_submission(pool, id).await;
                return Err(PushError::new("PUSH_APPROVAL_FAILED", error.code()));
            }
        }
        lock(&self.proposals).insert(id, req);
        tracing::info!(component = "push", submission_id = %id, result = "proposed", "push.pull_request");
        Ok(id)
    }

    pub(crate) async fn decide_pull_request(
        &self,
        row: SubmissionRow,
        state: &str,
    ) -> Result<(), PushError> {
        let pool = self.pool()?;
        if row.state != "proposed" {
            return Ok(());
        }
        let origin: SubmissionOrigin =
            serde_json::from_str(&row.origin).map_err(|_| PushError::store(true, "origin"))?;
        match state {
            "approved" => {
                store::update_submission(pool, row.id, "approved", None, None, None, None).await?;
                let proposal = lock(&self.proposals).remove(&row.id);
                let proposal = match proposal {
                    Some(p) => p,
                    // After a restart the file bytes are reloaded from their own table.
                    None => match serde_json::from_str::<PullRequestProposal>(&row.proposal) {
                        Ok(mut p) => match &mut p.content {
                            PullRequestContent::Files { files } => {
                                let stored = store::get_pull_request_files(pool, row.id).await?;
                                if stored.is_empty() {
                                    return self
                                        .fail_submission(row.id, origin, "PUSH_OUTCOME_UNKNOWN")
                                        .await;
                                }
                                *files = stored;
                                p
                            }
                            _ => p,
                        },
                        Err(_) => {
                            return self
                                .fail_submission(row.id, origin, "PUSH_OUTCOME_UNKNOWN")
                                .await;
                        }
                    },
                };
                let _ = self.send_submission(row.id, &proposal).await;
                Ok(())
            }
            _ => {
                store::update_submission(pool, row.id, "rejected", None, None, None, None).await?;
                store::delete_pull_request_files(pool, row.id).await?;
                lock(&self.proposals).remove(&row.id);
                self.notify(PushNotice::PullRequestRejected {
                    submission: row.id,
                    origin,
                });
                Ok(())
            }
        }
    }

    async fn fail_submission(
        &self,
        id: SubmissionId,
        origin: SubmissionOrigin,
        code: &'static str,
    ) -> Result<(), PushError> {
        let pool = self.pool()?;
        store::update_submission(pool, id, "failed", None, None, None, Some(code)).await?;
        let _ = store::delete_pull_request_files(pool, id).await;
        tracing::info!(component = "push", submission_id = %id, origin_kind = origin.kind(), result = code, "push.pull_request");
        self.notify(PushNotice::PullRequestFailed {
            submission: id,
            origin,
            code,
        });
        Ok(())
    }

    /// F.2: sends an approved submission; every outcome is reported through a notice and returned.
    // spec: ordin-desktop/components/PushService#SA-RECON-10
    async fn send_submission(
        &self,
        id: SubmissionId,
        req: &PullRequestProposal,
    ) -> Result<PullRequestRef, PushError> {
        let pool = self.pool()?;
        let key = ExternalActionKey::new();
        let destination = format!("{}/{}#{}", req.host, req.repository, req.branch);
        if let Err(error) = self
            .deps
            .journal
            .begin_external_action(NewExternalAction {
                action_key: key,
                run_id: None,
                kind: ExternalActionKind::PullRequestOpened,
                destination,
            })
            .await
        {
            let code = crate::error::classify_send(error.code());
            self.fail_submission(id, req.origin.clone(), code).await?;
            return Err(PushError::new(code, error.code()));
        }
        store::update_submission(pool, id, "approved", None, Some(key), None, None).await?;
        let outcome = self.push_content(req).await;
        let reference = match outcome {
            Ok(()) => match &req.existing {
                Some(existing) => Ok(existing.clone()),
                None => self
                    .deps
                    .github
                    .open_pull_request(OpenPullRequest {
                        host: req.host.clone(),
                        repository: req.repository.clone(),
                        head: req.branch.clone(),
                        base: req.base.clone(),
                        title: req.title.clone(),
                        body: req.body.clone(),
                        labels: req.labels.clone(),
                        reviewers: req.reviewers.clone(),
                    })
                    .await
                    .map_err(|e| github_code(&e)),
            },
            Err(code) => Err(code),
        };
        match reference {
            Ok(reference) => {
                if let Err(error) = self.deps.journal.confirm_external_action(key).await {
                    tracing::warn!(
                        component = "push",
                        code = error.code(),
                        "push.confirm_failed"
                    );
                }
                store::update_submission(pool, id, "sent", None, None, Some(&reference), None)
                    .await?;
                let _ = store::delete_pull_request_files(pool, id).await;
                tracing::info!(component = "push", submission_id = %id, origin_kind = req.origin.kind(), result = "sent", number = reference.number, "push.pull_request");
                self.notify(PushNotice::PullRequestOpened {
                    submission: id,
                    origin: req.origin.clone(),
                    pull_request: reference.clone(),
                });
                Ok(reference)
            }
            Err(code) => {
                self.fail_submission(id, req.origin.clone(), code).await?;
                Err(PushError::new(code, "pull request"))
            }
        }
    }

    /// The branch's content reaches the host: a worktree commit and push, or files through the API.
    async fn push_content(&self, req: &PullRequestProposal) -> Result<(), &'static str> {
        match &req.content {
            PullRequestContent::Worktree { path, paths } => {
                let credential = self
                    .deps
                    .github
                    .git_credentials(&req.host)
                    .await
                    .map_err(|e| github_code(&e))?;
                let creds = Creds {
                    username: credential.username.clone(),
                    token: credential.token.expose().to_owned(),
                };
                let (folder, paths, title) = (path.clone(), paths.clone(), req.title.clone());
                let refspec = format!("HEAD:refs/heads/{}", req.branch);
                let existing = req.existing.is_some();
                let pushed = tokio::time::timeout(
                    gitops::GIT_TIMEOUT,
                    tokio::task::spawn_blocking(move || -> Result<(), GitStep> {
                        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
                        gitops::commit_paths(&folder, None, &refs, &title)?;
                        let repo = git2::Repository::open(&folder)
                            .map_err(|e| GitFailure::Other(e.message().to_owned()))?;
                        gitops::push_refspec(&repo, "origin", &refspec, Some(&creds))?;
                        Ok(())
                    }),
                )
                .await;
                match pushed {
                    Ok(Ok(Ok(()))) => Ok(()),
                    Ok(Ok(Err(GitStep::IdentityMissing))) => Err("PUSH_GIT_IDENTITY_MISSING"),
                    Ok(Ok(Err(GitStep::WrongBranch))) => Err("PUSH_GIT_BRANCH_UNKNOWN"),
                    // A rejection on an existing pull request's branch: the review moved.
                    Ok(Ok(Err(GitStep::Failed(GitFailure::NonFastForward)))) if existing => {
                        Err("PUSH_BRANCH_MOVED")
                    }
                    Ok(Ok(Err(GitStep::Failed(GitFailure::NonFastForward)))) => {
                        Err("PUSH_DESTINATION_REFUSED")
                    }
                    Ok(Ok(Err(GitStep::Failed(GitFailure::Forbidden)))) => {
                        Err("PUSH_NO_REVIEW_RIGHT")
                    }
                    Ok(Ok(Err(GitStep::Failed(other)))) => Err(other.code()),
                    Ok(Err(_)) => Err("PUSH_DESTINATION_REFUSED"),
                    Err(_) => Err("PUSH_OUTCOME_UNKNOWN"),
                }
            }
            PullRequestContent::Files { files } => self
                .deps
                .github
                .create_branch_with_files(BranchWithFiles {
                    host: req.host.clone(),
                    repository: req.repository.clone(),
                    base: req.base.clone(),
                    branch: req.branch.clone(),
                    message: req.title.clone(),
                    files: files.clone(),
                })
                .await
                .map(|_| ())
                .map_err(|e| github_code(&e)),
        }
    }

    /// F.2: the card was approved elsewhere (the Handover Service); sends at once.
    pub async fn send_approved_pull_request(
        &self,
        req: ApprovedPullRequest,
    ) -> Result<PullRequestRef, PushError> {
        let pool = self.pool()?;
        let approved = self
            .deps
            .approvals
            .get(req.approval_id)
            .await
            .map(|r| r.state == ApprovalState::Approved)
            .unwrap_or(false);
        if !approved {
            return Err(PushError::new("PUSH_APPROVAL_MISSING", "no approved card"));
        }
        let p = &req.proposal;
        if !(1..=256).contains(&p.title.chars().count())
            || !valid_ref(&p.branch)
            || p.base.is_empty()
        {
            return Err(PushError::new(
                "PUSH_REQUEST_INVALID",
                "pull request proposal",
            ));
        }
        let id = SubmissionId::new();
        let origin =
            serde_json::to_string(&p.origin).map_err(|_| PushError::store(false, "origin"))?;
        store::insert_submission(pool, id, p.workspace_id, &origin, &stored_form(p)?).await?;
        store::update_submission(
            pool,
            id,
            "approved",
            Some(req.approval_id),
            None,
            None,
            None,
        )
        .await?;
        self.send_submission(id, p).await
    }

    /// G.3: submissions approved whose pull request is not recorded.
    pub(crate) async fn recover_pull_requests(&self) -> Result<(), PushError> {
        let pool = self.pool()?;
        for row in store::submissions_in_state(pool, "approved").await? {
            if row.pull_request.is_some() {
                continue;
            }
            let Ok(proposal) = serde_json::from_str::<PullRequestProposal>(&row.proposal) else {
                continue;
            };
            let found = self
                .deps
                .github
                .pull_request_for_branch(&proposal.host, &proposal.repository, &proposal.branch)
                .await;
            match found {
                Ok(Some(reference)) => {
                    if let Some(key) = row.action_key {
                        let _ = self.deps.journal.confirm_external_action(key).await;
                    }
                    store::update_submission(
                        pool,
                        row.id,
                        "sent",
                        None,
                        None,
                        Some(&reference),
                        None,
                    )
                    .await?;
                    tracing::info!(component = "push", action_key = ?row.action_key, origin_kind = proposal.origin.kind(), "push.recovered_arrived");
                    self.notify(PushNotice::PullRequestOpened {
                        submission: row.id,
                        origin: proposal.origin,
                        pull_request: reference,
                    });
                }
                _ => {
                    tracing::info!(component = "push", action_key = ?row.action_key, origin_kind = proposal.origin.kind(), "push.unknown_outcome");
                    self.fail_submission(row.id, proposal.origin, "PUSH_OUTCOME_UNKNOWN")
                        .await?;
                }
            }
        }
        Ok(())
    }

    // ---- I. Team knowledge shares ----

    /// I.1: stores the submission and raises the card; returns the card's approval id.
    pub async fn propose_team_share(&self, p: TeamShareProposal) -> Result<ApprovalId, PushError> {
        let pool = self.pool()?;
        let hex = p.commit.len() == 40
            && p.commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !(1..=256).contains(&p.title.chars().count())
            || p.body.chars().count() > 65_536
            || !valid_ref(&p.branch)
            || p.review_branch.as_deref().is_some_and(|b| !valid_ref(b))
            || !hex
            || !card_valid(&p.card)
        {
            return Err(PushError::new(
                "PUSH_REQUEST_INVALID",
                "team share proposal",
            ));
        }
        if store::get_team_share(pool, p.share_id).await?.is_some() {
            return Err(PushError::new(
                "PUSH_TEAM_SHARE_STATE",
                "share already stored",
            ));
        }
        let commit = p.commit.clone();
        let clone = p.clone_path.clone();
        let present = tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open_bare(&clone).ok()?;
            let oid = git2::Oid::from_str(&commit).ok()?;
            let found = repo.find_commit(oid).is_ok();
            Some(found)
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(false);
        if !present {
            return Err(PushError::new(
                "PUSH_TEAM_COMMIT_MISSING",
                "commit not in the clone",
            ));
        }
        store::insert_team_share(pool, &p).await?;
        let mut request = request_from(
            p.workspace_id,
            &p.card,
            text::TEAM_ACTION,
            format!("{}/{}", p.host, p.repository),
        );
        request.content = None;
        request.show_details = false;
        match self.deps.approvals.request(request).await {
            Ok(card) => {
                store::update_team_share(
                    pool,
                    p.share_id,
                    TeamUpdate {
                        state: "proposed",
                        approval: Some(card.approval_id),
                        commit: None,
                        key: None,
                        pull_request: None,
                        refused_keys: None,
                        moves: None,
                        result_code: None,
                    },
                )
                .await?;
                tracing::info!(component = "push", share_id = %p.share_id, step = "proposed", "push.team_share");
                Ok(card.approval_id)
            }
            Err(error) => {
                let _ = store::delete_team_share(pool, p.share_id).await;
                Err(PushError::new("PUSH_APPROVAL_FAILED", error.code()))
            }
        }
    }

    pub(crate) async fn decide_team_share(
        &self,
        row: TeamShareRow,
        state: &str,
    ) -> Result<(), PushError> {
        // An `expired` decision that follows a withdrawal is ignored (no TeamShareRejected).
        if row.state != "proposed" {
            return Ok(());
        }
        tracing::info!(component = "push", share_id = %row.share_id, step = "decided", result = state, "push.team_share");
        if state == "approved" {
            self.team_update(row.share_id, "approved", None, None, None, None, None, None)
                .await?;
            self.team_send(row.share_id).await
        } else {
            self.team_update(row.share_id, "rejected", None, None, None, None, None, None)
                .await?;
            self.notify(PushNotice::TeamShareRejected {
                share_id: row.share_id,
            });
            Ok(())
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn team_update(
        &self,
        id: TeamShareId,
        state: &str,
        commit: Option<&str>,
        key: Option<ExternalActionKey>,
        pull_request: Option<&PullRequestRef>,
        refused: Option<&[ExternalActionKey]>,
        moves: Option<u32>,
        code: Option<&str>,
    ) -> Result<(), PushError> {
        store::update_team_share(
            self.pool()?,
            id,
            TeamUpdate {
                state,
                approval: None,
                commit,
                key,
                pull_request,
                refused_keys: refused,
                moves,
                result_code: code,
            },
        )
        .await
    }

    /// I.6: the failure outcome of a team share send.
    async fn team_failed(&self, row: &TeamShareRow, code: &'static str) -> Result<(), PushError> {
        self.team_update(
            row.share_id,
            "failed",
            None,
            None,
            None,
            None,
            None,
            Some(code),
        )
        .await?;
        tracing::info!(component = "push", share_id = %row.share_id, step = "failed", result = code, moves = row.moves, "push.team_share");
        self.notify(PushNotice::TeamShareFailed {
            share_id: row.share_id,
            code,
        });
        if PushError::new(code, "").action.is_some() {
            self.needs_you(row.workspace_id).await;
        }
        Ok(())
    }

    /// I.3: sends the commit, direct or to a review branch with a pull request.
    pub(crate) async fn team_send(&self, id: TeamShareId) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(row) = store::get_team_share(pool, id).await? else {
            return Err(PushError::new("PUSH_TEAM_SHARE_UNKNOWN", "no such share"));
        };
        let key = ExternalActionKey::new();
        self.team_update(id, "sending", None, Some(key), None, None, None, None)
            .await?;
        let review = row.review_branch.clone();
        let branch = row.branch.clone();
        let target = review.clone().unwrap_or_else(|| branch.clone());
        let kind = if review.is_some() {
            ExternalActionKind::PullRequestOpened
        } else {
            ExternalActionKind::OriginPush
        };
        if let Err(error) = self
            .deps
            .journal
            .begin_external_action(NewExternalAction {
                action_key: key,
                run_id: None,
                kind,
                destination: format!("{}/{}#{}", row.host, row.repository, target),
            })
            .await
        {
            let code = crate::error::classify_send(error.code());
            return self.team_failed(&row, code).await;
        }
        let credential = match self.deps.github.git_credentials(&row.host).await {
            Ok(c) => Creds {
                username: c.username.clone(),
                token: c.token.expose().to_owned(),
            },
            Err(error) => return self.team_failed(&row, github_code(&error)).await,
        };
        let (clone, refspec) = (
            PathBuf::from(&row.clone_path),
            format!("{}:refs/heads/{target}", row.commit_id),
        );
        let pushed = tokio::time::timeout(
            gitops::GIT_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let repo = git2::Repository::open_bare(&clone)
                    .map_err(|e| GitFailure::Other(e.message().to_owned()))?;
                gitops::push_refspec(&repo, "origin", &refspec, Some(&credential))
            }),
        )
        .await;
        let failure = match pushed {
            Ok(Ok(Ok(()))) => None,
            Ok(Ok(Err(failure))) => Some(failure),
            Ok(Err(_)) => Some(GitFailure::Other("task".to_owned())),
            Err(_) => Some(GitFailure::Timeout),
        };
        match failure {
            None => {}
            Some(GitFailure::NonFastForward) if review.is_none() => {
                // I.4: nothing was sent; the refused key is remembered and the service waits.
                let mut refused = row.refused_keys.clone();
                refused.push(key);
                let moves = (row.moves + 1).min(3);
                self.team_update(
                    id,
                    "approved",
                    None,
                    None,
                    None,
                    Some(&refused),
                    Some(moves),
                    None,
                )
                .await?;
                tracing::info!(component = "push", share_id = %id, step = "moved", moves, "push.team_share");
                self.notify(PushNotice::TeamShareMoved { share_id: id });
                return Ok(());
            }
            Some(GitFailure::NonFastForward) => {
                return self.team_failed(&row, "PUSH_BRANCH_MOVED").await
            }
            Some(GitFailure::Forbidden) if review.is_some() => {
                return self.team_failed(&row, "PUSH_NO_REVIEW_RIGHT").await
            }
            Some(other) => return self.team_failed(&row, other.code()).await,
        }
        let mut reference = None;
        if let Some(rb) = &review {
            match self
                .deps
                .github
                .open_pull_request(OpenPullRequest {
                    host: row.host.clone(),
                    repository: row.repository.clone(),
                    head: rb.clone(),
                    base: branch.clone(),
                    title: row.title.clone(),
                    body: row.body.clone(),
                    labels: Vec::new(),
                    reviewers: Vec::new(),
                })
                .await
            {
                Ok(r) => reference = Some(r),
                Err(error) => return self.team_failed(&row, github_code(&error)).await,
            }
        }
        if let Err(error) = self.deps.journal.confirm_external_action(key).await {
            tracing::warn!(
                component = "push",
                code = error.code(),
                "push.confirm_failed"
            );
        }
        self.team_update(id, "sent", None, None, reference.as_ref(), None, None, None)
            .await?;
        tracing::info!(component = "push", share_id = %id, step = "sent", moves = row.moves, "push.team_share");
        self.notify(PushNotice::TeamShareSent {
            share_id: id,
            commit: row.commit_id.clone(),
            pull_request: reference,
        });
        Ok(())
    }

    /// I.5: after `TeamShareMoved`, sends the rebuilt commit under the same approval.
    pub async fn resend_team_share(
        &self,
        share_id: TeamShareId,
        commit: String,
    ) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(row) = store::get_team_share(pool, share_id).await? else {
            return Err(PushError::new("PUSH_TEAM_SHARE_UNKNOWN", "no such share"));
        };
        if row.state != "approved" || row.moves == 0 || row.moves >= 3 {
            return Err(PushError::new(
                "PUSH_TEAM_SHARE_STATE",
                "cannot resend from this state",
            ));
        }
        let hex = commit.len() == 40
            && commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let (clone, probe) = (PathBuf::from(&row.clone_path), commit.clone());
        let present = hex
            && tokio::task::spawn_blocking(move || {
                let repo = git2::Repository::open_bare(&clone).ok()?;
                let oid = git2::Oid::from_str(&probe).ok()?;
                let found = repo.find_commit(oid).is_ok();
                Some(found)
            })
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
        if !present {
            return Err(PushError::new(
                "PUSH_TEAM_COMMIT_MISSING",
                "commit not in the clone",
            ));
        }
        let approved = match row.approval_id {
            Some(approval) => self
                .deps
                .approvals
                .get(approval)
                .await
                .map(|r| r.state == ApprovalState::Approved)
                .unwrap_or(false),
            None => false,
        };
        if !approved {
            return Err(PushError::new("PUSH_APPROVAL_MISSING", "no approved card"));
        }
        self.team_update(
            share_id,
            "approved",
            Some(&commit),
            None,
            None,
            None,
            None,
            None,
        )
        .await?;
        self.team_send(share_id).await
    }

    /// I.8: before the decision only.
    pub async fn withdraw_team_share(&self, share_id: TeamShareId) -> Result<(), PushError> {
        let pool = self.pool()?;
        let Some(row) = store::get_team_share(pool, share_id).await? else {
            return Err(PushError::new("PUSH_TEAM_SHARE_UNKNOWN", "no such share"));
        };
        if row.state != "proposed" {
            return Err(PushError::new(
                "PUSH_TEAM_SHARE_STATE",
                "not waiting for a decision",
            ));
        }
        // Withdrawn first, so the `expired` decision that follows is ignored.
        self.team_update(share_id, "withdrawn", None, None, None, None, None, None)
            .await?;
        if let Some(approval) = row.approval_id {
            if let Err(error) = self.deps.approvals.withdraw(approval).await {
                tracing::warn!(
                    component = "push",
                    code = error.code(),
                    "push.withdraw_failed"
                );
            }
        }
        tracing::info!(component = "push", share_id = %share_id, step = "withdrawn", "push.team_share");
        Ok(())
    }

    /// I.7: a share left `sending`: fetches the branch and looks for the commit.
    pub(crate) async fn recover_team_shares(&self) -> Result<(), PushError> {
        let pool = self.pool()?;
        for row in store::team_shares_in_state(pool, "sending").await? {
            // The answer of a refused attempt is known.
            if row
                .action_key
                .is_some_and(|k| row.refused_keys.contains(&k))
            {
                continue;
            }
            let target = row
                .review_branch
                .clone()
                .unwrap_or_else(|| row.branch.clone());
            let creds = self
                .deps
                .github
                .git_credentials(&row.host)
                .await
                .ok()
                .map(|c| Creds {
                    username: c.username.clone(),
                    token: c.token.expose().to_owned(),
                });
            let (clone, commit, branch) = (
                PathBuf::from(&row.clone_path),
                row.commit_id.clone(),
                target.clone(),
            );
            let arrived = tokio::time::timeout(
                gitops::GIT_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    let repo = git2::Repository::open_bare(&clone).ok()?;
                    gitops::fetch_branch(&repo, "origin", &branch, creds.as_ref()).ok()?;
                    let tip = gitops::remote_tip(&repo, "origin", &branch)?;
                    Some(gitops::contains_commit(
                        &repo,
                        tip,
                        git2::Oid::from_str(&commit).ok()?,
                    ))
                }),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .unwrap_or(false);
            if !arrived {
                tracing::info!(component = "push", action_key = ?row.action_key, "push.unknown_outcome");
                self.team_failed(&row, "PUSH_OUTCOME_UNKNOWN").await?;
                continue;
            }
            let mut reference = None;
            if let Some(rb) = &row.review_branch {
                match self
                    .deps
                    .github
                    .pull_request_for_branch(&row.host, &row.repository, rb)
                    .await
                {
                    Ok(Some(r)) => reference = Some(r),
                    _ => {
                        self.team_failed(&row, "PUSH_OUTCOME_UNKNOWN").await?;
                        continue;
                    }
                }
            }
            self.team_update(
                row.share_id,
                "sent",
                None,
                None,
                reference.as_ref(),
                None,
                None,
                None,
            )
            .await?;
            tracing::info!(component = "push", share_id = %row.share_id, step = "recovered", "push.team_share");
            self.notify(PushNotice::TeamShareSent {
                share_id: row.share_id,
                commit: row.commit_id.clone(),
                pull_request: reference,
            });
        }
        Ok(())
    }
}
