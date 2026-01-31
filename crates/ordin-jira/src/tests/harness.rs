//! The test harness: an adapter on a Jira stand-in, an Identity double and a real Journal.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ordin_identity::{AccessToken, CredError, JiraAccess, JiraCredentialMethod, SecretString};
use ordin_journal::Journal;
use ordin_types::domain::runs::ExternalActionState;
use ordin_types::{CredentialId, ExternalActionKey};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::support::fixtures::{EMAIL, SITE, TOKEN};
use super::support::{open_journal, TempDir};
use crate::client::Credentials;
use crate::{JiraAdapter, JiraEndpoints};

/// What the double's `jira_access` answers.
pub(crate) enum Mode {
    ApiToken,
    Oauth,
    NoCredential,
    Error(CredError),
}

/// (site, email, token, account id, display name) as `save_jira_token` received them.
pub(crate) type Saved = (String, String, String, String, String);

pub(crate) struct FakeCreds {
    pub mode: Mutex<Mode>,
    pub site: Mutex<String>,
    pub credential_id: CredentialId,
    pub refreshes: AtomicUsize,
    pub refused: Mutex<Vec<(CredentialId, String)>>,
    pub saved: Mutex<Vec<Saved>>,
    pub save_error: Mutex<Option<CredError>>,
}

impl FakeCreds {
    pub fn new(mode: Mode) -> Arc<FakeCreds> {
        Arc::new(FakeCreds {
            mode: Mutex::new(mode),
            site: Mutex::new(SITE.to_owned()),
            credential_id: CredentialId::new(),
            refreshes: AtomicUsize::new(0),
            refused: Mutex::new(Vec::new()),
            saved: Mutex::new(Vec::new()),
            save_error: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Credentials for FakeCreds {
    async fn jira_access(&self) -> Result<Option<JiraAccess>, CredError> {
        let site = self.site.lock().unwrap().clone();
        let token = || AccessToken {
            token: SecretString::new(TOKEN.to_owned()),
            expires_at: None,
            credential_id: self.credential_id,
        };
        match &*self.mode.lock().unwrap() {
            Mode::ApiToken => Ok(Some(JiraAccess {
                site,
                method: JiraCredentialMethod::ApiToken {
                    email: EMAIL.to_owned(),
                },
                token: token(),
            })),
            Mode::Oauth => Ok(Some(JiraAccess {
                site,
                method: JiraCredentialMethod::Oauth,
                token: token(),
            })),
            Mode::NoCredential => Ok(None),
            Mode::Error(CredError::Retryable { code }) => Err(CredError::Retryable { code }),
            Mode::Error(CredError::Permanent { code }) => Err(CredError::Permanent { code }),
        }
    }

    async fn refresh_silently(&self) -> Result<bool, CredError> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    async fn report_refused(
        &self,
        credential_id: CredentialId,
        reason_code: &str,
    ) -> Result<(), CredError> {
        self.refused
            .lock()
            .unwrap()
            .push((credential_id, reason_code.to_owned()));
        Ok(())
    }

    async fn save_jira_token(
        &self,
        site: &str,
        email: &str,
        token: SecretString,
        account_id: &str,
        display_name: &str,
    ) -> Result<(), CredError> {
        if let Some(error) = self.save_error.lock().unwrap().take() {
            return Err(error);
        }
        self.saved.lock().unwrap().push((
            site.to_owned(),
            email.to_owned(),
            token.expose().to_owned(),
            account_id.to_owned(),
            display_name.to_owned(),
        ));
        Ok(())
    }
}

pub(crate) struct Fixture {
    pub adapter: Arc<JiraAdapter>,
    pub mock: MockServer,
    pub journal: Arc<Journal>,
    pub creds: Arc<FakeCreds>,
    pub dir: TempDir,
}

impl Fixture {
    pub async fn new() -> Fixture {
        Self::with(Mode::ApiToken, None).await
    }

    pub async fn with(mode: Mode, timeout: Option<std::time::Duration>) -> Fixture {
        let dir = TempDir::new();
        let journal = open_journal(&dir).await;
        let mock = MockServer::start().await;
        let creds = FakeCreds::new(mode);
        let adapter = JiraAdapter::from_parts(
            Arc::clone(&journal),
            Arc::clone(&creds) as Arc<dyn Credentials>,
            reqwest::Client::new(),
            JiraEndpoints {
                site_base_url: Some(mock.uri()),
                gateway_base_url: Some(mock.uri()),
                request_timeout: timeout,
            },
        );
        Fixture {
            adapter,
            mock,
            journal,
            creds,
            dir,
        }
    }

    /// Answers `http_method path` with `status` and a JSON body.
    pub async fn reply(&self, http_method: &str, at: &str, status: u16, body: Value) {
        Mock::given(method(http_method))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&self.mock)
            .await;
    }

    /// Answers with `status` and no body.
    pub async fn status(&self, http_method: &str, at: &str, status: u16) {
        Mock::given(method(http_method))
            .and(path(at))
            .respond_with(ResponseTemplate::new(status))
            .mount(&self.mock)
            .await;
    }

    /// Every request the stand-in received for `http_method path`.
    pub async fn received(&self, http_method: &str, at: &str) -> Vec<Request> {
        self.mock
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.method.as_str() == http_method && r.url.path() == at)
            .collect()
    }

    pub async fn total_requests(&self) -> usize {
        self.mock
            .received_requests()
            .await
            .unwrap_or_default()
            .len()
    }

    pub async fn action_state(&self, key: ExternalActionKey) -> Option<ExternalActionState> {
        self.journal
            .external_action(key)
            .await
            .unwrap()
            .map(|a| a.state)
    }
}

pub(crate) fn issue_ref(key: &str) -> crate::JiraIssueRef {
    crate::JiraIssueRef {
        site: SITE.to_owned(),
        key: key.to_owned(),
    }
}

pub(crate) fn project_ref(key: &str) -> crate::JiraProjectRef {
    crate::JiraProjectRef {
        site: SITE.to_owned(),
        key: key.to_owned(),
    }
}

pub(crate) fn body_json(request: &Request) -> Value {
    serde_json::from_slice(&request.body).unwrap()
}

/// The `Authorization` header of a request, as text.
pub(crate) fn authorization(request: &Request) -> String {
    request
        .headers
        .get("authorization")
        .map(|v| v.to_str().unwrap_or("").to_owned())
        .unwrap_or_default()
}
