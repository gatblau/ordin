//! Secrets the person types or pastes: mail passwords (Internal Logic F), a GitLab personal access
//! token (H.11, SA-CLOSE-01) and a Jira API token (§J.3), and `jira_access` (§J.4).

use chrono::NaiveDate;
use ordin_types::domain::identity::{Credential, CredentialKind};
use ordin_types::time::Timestamp;
use ordin_types::CredentialId;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::config::Program;
use crate::error::{code, fail, CredError};
use crate::oauth::Auth;
use crate::secret::{SecretName, SecretString};
use crate::service::{
    credential_secret_name, is_password_kind, new_row, short_hash, IdentityService, TokenBlob,
};
use crate::sign_in::{host_ok, jira_host_ok, GITLAB_SCOPES};
use crate::store::{kind_str, Meta, Row};
use crate::types::{
    AccessToken, JiraAccess, JiraCredentialMethod, MailPasswordKind, JIRA_OAUTH_SCOPES,
};

/// Mail password or app password length, and a pasted token's: 1–1024 bytes UTF-8.
const MAX_PASSWORD_BYTES: usize = 1024;

fn label_ok(label: &str, max: usize) -> Result<(), CredError> {
    let chars = label.chars().count();
    if chars == 0 || chars > max {
        return fail(code::LABEL_INVALID);
    }
    Ok(())
}

fn password_ok(secret: &str) -> Result<(), CredError> {
    if secret.is_empty() || secret.len() > MAX_PASSWORD_BYTES {
        return fail(code::PASSWORD_INVALID);
    }
    Ok(())
}

/// SPEC-GAP-7: `credential_meta.provider_host` is required, and F.1 names no value for a mail password.
/// The domain of the mail account is stored (the label itself when it has no `@`).
fn mail_host(label: &str) -> String {
    label
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .filter(|d| !d.is_empty())
        .unwrap_or(label)
        .chars()
        .take(255)
        .collect()
}

impl IdentityService {
    /// Stores the password the person typed once (Settings › Email or the add-email screen).
    /// Replaces the password of an existing active credential for the same account and clears its refusal mark.
    pub async fn save_mail_password(
        &self,
        account_label: &str,
        kind: MailPasswordKind,
        password: SecretString,
    ) -> Result<Credential, CredError> {
        label_ok(account_label, 254)?;
        password_ok(password.expose())?;
        let kind = match kind {
            MailPasswordKind::MailPassword => CredentialKind::MailServerPassword,
            MailPasswordKind::AppPassword => CredentialKind::MailServerAppPassword,
        };
        let existing = self.inner.active().into_iter().find(|r| {
            is_password_kind(r.credential.kind) && r.credential.account_label == account_label
        });
        let meta = Meta {
            provider_host: mail_host(account_label),
            scopes: Vec::new(),
            needs_sign_in: false,
            refused_at: None,
            last_error_code: None,
            display_name: None,
        };
        let (row, is_new, previous) = match existing {
            Some(existing) => {
                let previous = existing.credential.keychain_ref.clone();
                let mut row = existing.clone();
                row.credential.kind = kind;
                row.credential.keychain_ref = credential_secret_name(kind, row.credential.id);
                row.credential.refreshed_at = Some(Timestamp::now());
                row.meta = Meta {
                    last_error_code: existing.meta.last_error_code.clone(),
                    display_name: existing.meta.display_name.clone(),
                    ..meta
                };
                (row, false, Some(previous))
            }
            None => (
                new_row(
                    CredentialId::new(),
                    kind,
                    account_label.to_owned(),
                    Timestamp::now(),
                    None,
                    meta,
                ),
                true,
                None,
            ),
        };
        let credential = self
            .inner
            .persist(row, is_new, password.expose().as_bytes(), previous)
            .await?;
        log_completed(&credential, "password");
        Ok(credential)
    }

    /// Only the Mail Service's IMAP and SMTP adapter calls this.
    pub async fn mail_password(
        &self,
        credential_id: CredentialId,
    ) -> Result<SecretString, CredError> {
        let row = self.inner.row(credential_id)?;
        if !is_password_kind(row.credential.kind) {
            return fail(code::KIND_MISMATCH);
        }
        if row.meta.needs_sign_in {
            return fail(code::SIGN_IN_REQUIRED);
        }
        let name = SecretName::new(&row.credential.keychain_ref)?;
        let Some(bytes) = self.inner.secrets.get(&name).await? else {
            return fail(code::KEYCHAIN_MISSING);
        };
        let text =
            String::from_utf8(bytes.to_vec()).or_else(|_| fail(code::SECRET_STORE_CORRUPT))?;
        Ok(SecretString::new(text))
    }

    /// The server refused this credential. Marks it `needs_sign_in`; never retried here (ADR-071).
    pub async fn report_refused(
        &self,
        credential_id: CredentialId,
        reason_code: &str,
    ) -> Result<(), CredError> {
        let row = self.inner.row(credential_id)?;
        let well_formed = !reason_code.is_empty()
            && reason_code.len() <= 64
            && reason_code
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        let reason = if well_formed {
            reason_code
        } else {
            "UNSPECIFIED"
        };
        self.inner
            .mark_needs_sign_in(&row, reason, Some(Timestamp::now()), true)
            .await?;
        tracing::warn!(
            target: "ordin_identity",
            credential_id = credential_id.to_string().as_str(),
            kind = kind_str(row.credential.kind),
            reason_code = reason,
            "cred.refused"
        );
        Ok(())
    }

    /// GitLab without an OAuth application: stores the personal access token the person pasted,
    /// after checking it on `host` (H.11).
    // spec: ordin-desktop/components/IdentityAndCredentials#SA-CLOSE-01
    pub async fn save_gitlab_token(
        &self,
        host: &str,
        token: SecretString,
    ) -> Result<Credential, CredError> {
        host_ok(host)?;
        password_ok(token.expose())?;
        let http = &self.inner.http;
        let check_url = format!("https://{host}/api/v4/personal_access_tokens/self");
        let reply = http
            .get(&check_url, Auth::PrivateToken(token.expose()))
            .await?;
        let body = match reply.status {
            200..=299 => reply.body,
            500.. => return fail(code::PROVIDER_UNREACHABLE),
            _ => return fail(code::PROVIDER_REFUSED),
        };
        let refused = body.get("revoked").and_then(Value::as_bool) == Some(true)
            || body.get("active").and_then(Value::as_bool) == Some(false);
        if refused {
            return fail(code::PROVIDER_REFUSED);
        }
        let scopes: Vec<String> = body
            .get("scopes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if !GITLAB_SCOPES.iter().all(|s| scopes.iter().any(|g| g == s)) {
            return fail(code::SCOPE_MISSING);
        }
        let expires_at = body
            .get("expires_at")
            .and_then(Value::as_str)
            .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            .and_then(|day| day.and_hms_opt(0, 0, 0))
            .and_then(|at| Timestamp::from_unix_millis(at.and_utc().timestamp_millis()).ok());
        let user_url = format!("https://{host}/api/v4/user");
        let reply = http
            .get(&user_url, Auth::PrivateToken(token.expose()))
            .await?;
        let user = match reply.status {
            200..=299 => reply.body,
            500.. => return fail(code::PROVIDER_UNREACHABLE),
            _ => return fail(code::PROVIDER_REFUSED),
        };
        let label = user
            .get("username")
            .and_then(Value::as_str)
            .ok_or_else(|| CredError::of(code::PROVIDER_REFUSED))?
            .to_owned();
        label_ok(&label, 254)?;
        let display_name = user
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .map(|n| n.chars().take(254).collect());
        let blob = TokenBlob {
            access_token: token.expose().to_owned(),
            refresh_token: None,
            expires_at,
            scopes: scopes.clone(),
            token_type: "private_token".to_owned(),
            method: None,
            email: None,
            account_id: None,
            client_id: None,
            token_endpoint: None,
            client_secret: None,
        };
        let meta = Meta {
            provider_host: host.to_owned(),
            scopes,
            needs_sign_in: false,
            refused_at: None,
            last_error_code: None,
            display_name,
        };
        let existing = self.inner.active().into_iter().find(|r| {
            r.credential.kind == CredentialKind::Gitlab && r.credential.account_label == label
        });
        let credential = self
            .persist_pasted(
                existing,
                CredentialKind::Gitlab,
                label,
                expires_at,
                meta,
                &blob,
            )
            .await?;
        log_completed(&credential, "pasted_token");
        Ok(credential)
    }

    /// Keeps a pasted API token that the Jira Adapter has already checked with `GET /rest/api/3/myself`.
    pub async fn save_jira_token(
        &self,
        site: &str,
        email: &str,
        token: SecretString,
        account_id: &str,
        display_name: &str,
    ) -> Result<Credential, CredError> {
        if self.inner.program != Program::Desktop {
            return fail(code::FLOW_NOT_OFFERED);
        }
        jira_host_ok(site)?;
        let email_chars = email.chars().count();
        if !(3..=254).contains(&email_chars) || email.matches('@').count() != 1 {
            return fail(code::PASSWORD_INVALID);
        }
        password_ok(token.expose())?;
        label_ok(account_id, 128)?;
        label_ok(display_name, 254)?;
        let jira: Vec<Row> = self
            .inner
            .active()
            .into_iter()
            .filter(|r| r.credential.kind == CredentialKind::Jira)
            .collect();
        if jira.iter().any(|r| r.credential.account_label != site) {
            return fail(code::DUPLICATE);
        }
        let blob = TokenBlob {
            access_token: token.expose().to_owned(),
            refresh_token: None,
            expires_at: None,
            scopes: Vec::new(),
            token_type: "atlassian_api_token".to_owned(),
            method: Some("api_token".to_owned()),
            email: Some(email.to_owned()),
            account_id: Some(account_id.to_owned()),
            client_id: None,
            token_endpoint: None,
            client_secret: None,
        };
        let meta = Meta {
            provider_host: site.to_owned(),
            scopes: Vec::new(),
            needs_sign_in: false,
            refused_at: None,
            last_error_code: None,
            display_name: Some(display_name.to_owned()),
        };
        let existing = jira.into_iter().next();
        let credential = self
            .persist_pasted(
                existing,
                CredentialKind::Jira,
                site.to_owned(),
                None,
                meta,
                &blob,
            )
            .await?;
        log_completed(&credential, "pasted_token");
        Ok(credential)
    }

    /// The person's single active Jira credential. Ok(None) when no Jira credential exists.
    pub async fn jira_access(&self) -> Result<Option<JiraAccess>, CredError> {
        let row = match self.inner.select(CredentialKind::Jira, None) {
            Ok(row) => row,
            Err(e) if ordin_types::CodedError::code(&e) == code::SIGN_IN_REQUIRED => {
                return Ok(None)
            }
            Err(e) => return Err(e),
        };
        if row.meta.needs_sign_in {
            return fail(code::SIGN_IN_REQUIRED);
        }
        let blob = self.inner.read_blob(&row).await?;
        let site = row.credential.account_label.clone();
        if blob.method.as_deref() == Some("api_token") {
            let email = blob.email.clone().unwrap_or_default();
            return Ok(Some(JiraAccess {
                site,
                method: JiraCredentialMethod::ApiToken { email },
                token: AccessToken {
                    token: SecretString::new(blob.access_token.clone()),
                    expires_at: None,
                    credential_id: row.credential.id,
                },
            }));
        }
        if !JIRA_OAUTH_SCOPES
            .iter()
            .all(|s| row.meta.scopes.iter().any(|g| g == s))
        {
            return fail(code::SCOPE_MISSING);
        }
        let token = self.inner.token_of(&row).await?;
        Ok(Some(JiraAccess {
            site,
            method: JiraCredentialMethod::Oauth,
            token,
        }))
    }

    /// C.10 for a pasted token: an existing active row for the same account is updated in place (its
    /// secret replaced, its refusal mark cleared).
    async fn persist_pasted(
        &self,
        existing: Option<Row>,
        kind: CredentialKind,
        label: String,
        expires_at: Option<Timestamp>,
        meta: Meta,
        blob: &TokenBlob,
    ) -> Result<Credential, CredError> {
        let bytes = Zeroizing::new(serde_json::to_vec(blob).or_else(|_| fail(code::STORE_FAILED))?);
        let (row, is_new) = match existing {
            Some(existing) => {
                let mut row = existing.clone();
                row.credential.expires_at = expires_at;
                row.credential.refreshed_at = Some(Timestamp::now());
                row.meta = Meta {
                    last_error_code: existing.meta.last_error_code.clone(),
                    ..meta
                };
                (row, false)
            }
            None => (
                new_row(
                    CredentialId::new(),
                    kind,
                    label,
                    Timestamp::now(),
                    expires_at,
                    meta,
                ),
                true,
            ),
        };
        self.inner.persist(row, is_new, &bytes, None).await
    }
}

fn log_completed(credential: &Credential, method: &str) {
    tracing::info!(
        target: "ordin_identity",
        credential_id = credential.id.to_string().as_str(),
        kind = kind_str(credential.kind),
        account_hash = short_hash(&credential.account_label).as_str(),
        method = method,
        "cred.sign_in.completed"
    );
}
