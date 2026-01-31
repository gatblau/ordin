//! Linking a person's source-system accounts to a front door (ADR-115; amendment *Internal Logic*
//! PD-A). Runs off the question path: when a front door is connected, when the program starts, and
//! when the person signs in to or out of GitHub, GitLab or Confluence in ordin.

use std::sync::Arc;

use ordin_identity::{SignInChanged, SignInState};
use ordin_types::domain::identity::CredentialKind;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use tokio::sync::broadcast::error::RecvError;

use crate::client::{is_link_problem, Inner, COMPONENT, LINK_PROBLEM_SUFFIX};
use crate::error::KqError;
use crate::store::{self, ConnRow};
use crate::tokens::LinkCredential;
use crate::transport::read_plain;
use crate::types::*;

#[derive(Deserialize)]
struct LinkReply {
    #[serde(default)]
    linked: Vec<LinkedEntry>,
    #[serde(default)]
    refused: Vec<RefusedEntry>,
}

/// A `linked` entry is the system's name or an object holding it.
#[derive(Deserialize)]
#[serde(untagged)]
enum LinkedEntry {
    Name(SourceSystem),
    Object { system: SourceSystem },
}

impl LinkedEntry {
    fn system(&self) -> SourceSystem {
        match self {
            LinkedEntry::Name(system) | LinkedEntry::Object { system } => *system,
        }
    }
}

#[derive(Deserialize)]
struct RefusedEntry {
    system: SourceSystem,
    #[serde(default)]
    reason: String,
}

fn kind_of(system: SourceSystem) -> CredentialKind {
    match system {
        SourceSystem::Github => CredentialKind::Github,
        SourceSystem::Gitlab => CredentialKind::Gitlab,
        SourceSystem::Confluence => CredentialKind::Confluence,
        SourceSystem::Sharepoint => CredentialKind::Microsoft,
    }
}

fn system_of(kind: CredentialKind) -> Option<SourceSystem> {
    match kind {
        CredentialKind::Github => Some(SourceSystem::Github),
        CredentialKind::Gitlab => Some(SourceSystem::Gitlab),
        CredentialKind::Confluence => Some(SourceSystem::Confluence),
        _ => None,
    }
}

/// The active credential for one need: the matching kind, the host the front door names, and not
/// `needs_sign_in`.
// spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-PD-01
fn credential_for<'a>(
    credentials: &'a [LinkCredential],
    need: &AccountNeed,
) -> Option<&'a LinkCredential> {
    credentials.iter().find(|c| {
        c.kind == kind_of(need.system)
            && !c.needs_sign_in
            && c.host.eq_ignore_ascii_case(&need.host)
    })
}

impl Inner {
    fn needs_of(&self, row: &ConnRow) -> Vec<AccountNeed> {
        self.about_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&row.id).map(|e| e.accounts.clone()))
            .unwrap_or_default()
            .into_iter()
            // spec: ordin-desktop/components/IdentityAndCredentials#SA-ID-PD-03
            .filter(|need| need.system != SourceSystem::Sharepoint)
            .collect()
    }

    /// PD-A.2 to PD-A.5 for one connection; `only` limits it to one system (a sign-in change).
    /// Never fails: a network failure leaves every state as it was, to be retried at the next trigger.
    pub(crate) async fn link_connection(&self, row: &ConnRow, only: Option<SourceSystem>) {
        let needs: Vec<AccountNeed> = self
            .needs_of(row)
            .into_iter()
            .filter(|n| only.is_none_or(|s| s == n.system))
            .collect();
        if needs.is_empty() {
            return;
        }
        let (Ok(sign_in), Ok(tokens), Ok(pool)) =
            (self.person_sign_in(), self.token_source(), self.pool())
        else {
            return;
        };
        let Ok(credentials) = tokens.credentials().await else {
            return;
        };
        let mut entries: Vec<(SourceSystem, String)> = Vec::new();
        for need in &needs {
            let Some(credential) = credential_for(&credentials, need) else {
                let _ = store::upsert_account(
                    &pool,
                    row.id,
                    need.system,
                    AccountLinkState::NotSignedIn,
                )
                .await;
                continue;
            };
            let token = if need.system == SourceSystem::Confluence {
                tokens.confluence_read_me().await
            } else {
                tokens.token_by_id(credential.id).await
            };
            // A failed token read leaves the system out silently; it is retried at the next trigger.
            if let Ok(token) = token {
                entries.push((need.system, token.token.expose().to_owned()));
            }
        }
        if entries.is_empty() {
            return;
        }
        let body = json!({
            "accounts": entries
                .iter()
                .map(|(system, token)| json!({ "system": system.as_str(), "token": token }))
                .collect::<Vec<_>>()
        })
        .to_string();
        let Some(reply) = self.post_accounts(sign_in, row, body).await else {
            return;
        };
        let mut linked = 0usize;
        for entry in &reply.linked {
            let _ = store::upsert_account(&pool, row.id, entry.system(), AccountLinkState::Linked)
                .await;
            linked += 1;
        }
        for refusal in &reply.refused {
            match refusal.reason.as_str() {
                "not_accepted" => {
                    let _ = store::upsert_account(
                        &pool,
                        row.id,
                        refusal.system,
                        AccountLinkState::Refused,
                    )
                    .await;
                    let sentence = format!("{}{LINK_PROBLEM_SUFFIX}", refusal.system.display());
                    let _ = store::set_problem(&pool, row.id, Some(&sentence)).await;
                }
                "not_checked_here" => {
                    let _ = store::delete_account(&pool, row.id, refusal.system).await;
                }
                // `try_again` and anything else: the state is unchanged.
                _ => {}
            }
        }
        if reply.refused.is_empty() && linked > 0 {
            // Everything sent was accepted: an old refusal sentence no longer holds.
            if let Ok(Some(current)) = store::get(&pool, row.id).await {
                if current.last_problem.as_deref().is_some_and(is_link_problem) {
                    let _ = store::set_problem(&pool, row.id, None).await;
                }
            }
        }
        tracing::debug!(
            component = COMPONENT,
            connection_id = row.id.to_string().as_str(),
            linked,
            refused = reply.refused.len(),
            "kq_accounts_linked"
        );
    }

    /// `POST {url}/v1/knowledge/accounts`; 401 invalidates the audience token and retries once.
    async fn post_accounts(
        &self,
        sign_in: &CompanySignIn,
        row: &ConnRow,
        body: String,
    ) -> Option<LinkReply> {
        let tokens = self.token_source().ok()?;
        let mut retried = false;
        loop {
            let token = self.person_token(sign_in, &row.token_audience).await.ok()?;
            let request = self
                .transport
                .client(&row.url)
                .post(
                    self.transport
                        .address(format!("{}/v1/knowledge/accounts", row.url)),
                )
                .bearer_auth(token)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .timeout(self.timings.about_timeout)
                .body(body.clone());
            let plain = read_plain(request).await.ok()?;
            match plain.status {
                200 => return serde_json::from_slice(&plain.body).ok(),
                401 if !retried => {
                    retried = true;
                    tokens
                        .invalidate_audience(
                            &sign_in.issuer,
                            &sign_in.client_id,
                            &row.token_audience,
                        )
                        .await;
                }
                _ => return None,
            }
        }
    }

    /// `SignedOut` (PD-A.1 c): `DELETE {url}/v1/knowledge/accounts/{system}` on every live connection
    /// that lists the system; the state becomes `NotSignedIn` whether or not the front door was reached.
    async fn unlink_system(&self, pool: &SqlitePool, row: &ConnRow, system: SourceSystem) {
        let Ok(sign_in) = self.person_sign_in() else {
            return;
        };
        let Ok(tokens) = self.token_source() else {
            return;
        };
        let mut retried = false;
        while let Ok(token) = self.person_token(sign_in, &row.token_audience).await {
            let request = self
                .transport
                .client(&row.url)
                .delete(self.transport.address(format!(
                    "{}/v1/knowledge/accounts/{}",
                    row.url,
                    system.as_str()
                )))
                .bearer_auth(token)
                .timeout(self.timings.about_timeout);
            match read_plain(request).await {
                Ok(plain) if plain.status == 401 && !retried => {
                    retried = true;
                    tokens
                        .invalidate_audience(
                            &sign_in.issuer,
                            &sign_in.client_id,
                            &row.token_audience,
                        )
                        .await;
                }
                _ => break,
            }
        }
        let _ = store::upsert_account(pool, row.id, system, AccountLinkState::NotSignedIn).await;
    }

    async fn on_sign_in_change(&self, change: SignInChanged) {
        let Some(system) = system_of(change.credential_kind) else {
            return;
        };
        let Ok(pool) = self.pool() else { return };
        let Ok(rows) = store::all_live(&pool).await else {
            return;
        };
        for row in rows {
            if !self.needs_of(&row).iter().any(|n| n.system == system) {
                continue;
            }
            match change.state {
                SignInState::SignedIn => self.link_connection(&row, Some(system)).await,
                SignInState::SignedOut => self.unlink_system(&pool, &row, system).await,
                SignInState::NeedsSignIn => {}
            }
        }
    }

    /// A lagged receiver relinks every system of every live connection.
    async fn relink_all(&self) {
        let Ok(pool) = self.pool() else { return };
        let Ok(rows) = store::all_live(&pool).await else {
            return;
        };
        for row in rows {
            self.link_connection(&row, None).await;
        }
    }

    /// The task that follows the person's sign-ins without polling (Approach).
    pub(crate) async fn sign_in_loop(self: Arc<Self>) {
        let Ok(tokens) = self.token_source() else {
            return;
        };
        let mut changes = tokens.subscribe();
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => return,
                received = changes.recv() => match received {
                    Ok(change) => self.on_sign_in_change(change).await,
                    Err(RecvError::Lagged(_)) => self.relink_all().await,
                    Err(RecvError::Closed) => return,
                },
            }
        }
    }

    /// PD-A.6: one entry per `about.accounts` system, `NotSignedIn` by default.
    pub(crate) async fn accounts_view(
        &self,
        pool: &SqlitePool,
        row: &ConnRow,
    ) -> Result<Vec<ConnectionAccount>, KqError> {
        let stored = store::accounts_of(pool, row.id).await?;
        let needs = self.needs_of(row);
        let state_of = |system: SourceSystem| {
            stored
                .iter()
                .find(|(s, _)| *s == system)
                .map_or(AccountLinkState::NotSignedIn, |(_, state)| *state)
        };
        let mut view: Vec<ConnectionAccount> = Vec::new();
        if needs.is_empty() {
            view.extend(stored.iter().map(|(system, state)| ConnectionAccount {
                system: *system,
                state: *state,
            }));
        }
        for need in needs {
            if view.iter().all(|a| a.system != need.system) {
                view.push(ConnectionAccount {
                    system: need.system,
                    state: state_of(need.system),
                });
            }
        }
        Ok(view)
    }
}
