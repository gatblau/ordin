//! The rows of `DesktopServiceApi` whose owner takes an argument the screens never send, so the
//! program fills it (ServiceApi.md SVC-R1-1): `agents.installed_files` and `agents.part_page`
//! (SA-CLOSE-06: the agent's folder and its bundle directory) and `checker.check_installed` and
//! `checker.check_library_item` (Checker.md, *Service API commands*: the lock file and the source
//! scope).

use ordin_agent_model::RepositoryRef;
use ordin_identity::{ProviderConfig, SecretString, SignInMethod, SignInRequest};
use ordin_service_api::commands::agents::{InstalledFileInput, InstalledFilesInput, PartPageInput};
use ordin_service_api::commands::checker::{CheckInstalledInput, CheckLibraryItemInput};
use ordin_service_api::commands::confluence::{
    ConfluencePublishCheckedInput, ConfluencePublishCheckedResult,
};
use ordin_service_api::commands::files::FilesSaveEditInput;
use ordin_service_api::commands::identity::{
    CredentialView, IdentityImportClientCertificateResult, IdentitySaveGithubTokenInput,
    IdentitySaveGitlabTokenInput, IdentitySaveMailPasswordInput, IdentitySignInInput,
    IdentitySignInWaitInput, SignInChallenge,
};
use ordin_service_api::ServiceError;
use ordin_types::domain::identity::CredentialKind;
use zeroize::Zeroizing;

use super::{DesktopServiceApi, PICKER_UNAVAILABLE};

/// The largest certificate or key file `identity.import_client_certificate` reads (OrdinDesktop.md
/// DSK-R1-1).
const CERTIFICATE_FILE_LIMIT: usize = 64 * 1024;
const NOT_CONFIGURED: &str = "CRED_PROVIDER_NOT_CONFIGURED";
const FLOW_NOT_OFFERED: &str = "CRED_FLOW_NOT_OFFERED";
const CERTIFICATE_INVALID: &str = "CRED_CERTIFICATE_INVALID";

/// The provider and the scopes the caller sends for one kind (IdentityAndCredentials.md ID-R1-1,
/// ID-R1-4 to ID-R1-6), or the code that refuses it. Pure: it reads the profile only.
// spec: ordin-desktop/components/IdentityAndCredentials#ID-R1-4
// spec: ordin-desktop/components/IdentityAndCredentials#ID-R1-5
// spec: ordin-desktop/components/IdentityAndCredentials#ID-R1-7
pub(crate) fn sign_in_provider(
    document: &ordin_organisation_profile::OrganisationProfileDocument,
    kind: CredentialKind,
    site: Option<&str>,
    mail_account: Option<&str>,
) -> Result<(ProviderConfig, Vec<String>), &'static str> {
    let owned = |scopes: &[&str]| scopes.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    Ok(match kind {
        CredentialKind::Microsoft => {
            let section = document.microsoft.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Microsoft {
                    tenant_id: section.tenant_id.clone(),
                    client_id: section.client_id.clone(),
                },
                owned(&ordin_sharepoint::SCOPES),
            )
        }
        CredentialKind::Confluence => {
            let section = document.confluence.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Confluence {
                    site: section.site.clone(),
                    client_id: section.oauth_client_id.clone(),
                },
                owned(&ordin_confluence::SCOPES),
            )
        }
        CredentialKind::Jira => {
            let site = site.ok_or(NOT_CONFIGURED)?;
            let section = document.confluence.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Jira {
                    site: site.to_owned(),
                    client_id: section.oauth_client_id.clone(),
                },
                Vec::new(),
            )
        }
        CredentialKind::Slack => {
            let section = document.slack.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Slack {
                    workspace: section.workspace.clone().ok_or(NOT_CONFIGURED)?,
                    client_id: section.client_id.clone().ok_or(NOT_CONFIGURED)?,
                },
                Vec::new(),
            )
        }
        CredentialKind::Gitlab => {
            let section = document.gitlab.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Gitlab {
                    host: section.host.clone(),
                    client_id: section.client_id.clone().ok_or(NOT_CONFIGURED)?,
                },
                Vec::new(),
            )
        }
        CredentialKind::Oidc => {
            let section = document.identity_provider.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Oidc {
                    issuer: section.issuer.clone(),
                    client_id: section.client_id.clone(),
                    scopes: section.scopes.clone(),
                },
                section.scopes.clone(),
            )
        }
        // GitHub App: the client id is the build's, checked by Identity (ID-R1-4).
        CredentialKind::Github => (
            ProviderConfig::Github {
                host: "github.com".to_owned(),
            },
            Vec::new(),
        ),
        CredentialKind::Google => {
            let section = document.google.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::Google {
                    client_id: section.client_id.clone(),
                    client_secret: section.client_secret.clone(),
                },
                Vec::new(),
            )
        }
        // The organisation's identity provider and the person's mail address (ID-R1-7); the
        // Service API's input check has already required the address for this kind.
        CredentialKind::MailServerSingleSignOn => {
            let section = document.identity_provider.as_ref().ok_or(NOT_CONFIGURED)?;
            (
                ProviderConfig::MailServerSso {
                    issuer: section.issuer.clone(),
                    client_id: section.client_id.clone(),
                    scopes: section.scopes.clone(),
                    mail_account: mail_account.ok_or(NOT_CONFIGURED)?.to_owned(),
                },
                section.scopes.clone(),
            )
        }
        // The two mail password kinds are saved, not signed in (`identity.save_mail_password`).
        CredentialKind::MailServerPassword | CredentialKind::MailServerAppPassword => {
            return Err(FLOW_NOT_OFFERED)
        }
    })
}

impl DesktopServiceApi {
    /// The agent's bundle directory: `resolver.open_bundle(installers.record(id).plan_hash)`.
    async fn bundle_of(
        &self,
        id: ordin_types::AgentInstanceId,
    ) -> Result<ordin_resolver::Bundle, ServiceError> {
        let installers = self.handle(&self.modules.installers)?;
        let resolver = self.handle(&self.modules.resolver)?;
        let record = installers
            .record(id)
            .await
            .map_err(|e| self.module_error(e))?;
        resolver
            .open_bundle(&record.plan_hash)
            .map_err(|e| self.module_error(e))
    }

    /// `agents.installed_files`: `installed_files(agent_instance_id, folder_path)` with the folder
    /// from `PackageCatalogue::get(id)`.
    // spec: ordin-desktop/components/ServiceApi#SA-CLOSE-06
    pub(super) async fn agents_installed_files(
        &self,
        input: InstalledFilesInput,
    ) -> Result<Vec<ordin_service_api::commands::agents::InstalledFileEntry>, ServiceError> {
        let inspector = self.handle(&self.modules.inspector)?;
        let packages = self.handle(&self.modules.packages)?;
        let id = self.restate(input.agent_instance_id)?;
        let agent = packages.get(id).await.map_err(|e| self.module_error(e))?;
        let folder = std::path::PathBuf::from(agent.instance.agent_folder_path);
        let files = inspector
            .installed_files(id, &folder)
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(files)
    }

    /// `agents.installed_file`: `installed_file(agent_instance_id, folder_path, path)` with the folder
    /// from `PackageCatalogue::get(id)`; a path outside the folder is refused by the inspector.
    pub(super) async fn agents_installed_file(
        &self,
        input: InstalledFileInput,
    ) -> Result<ordin_service_api::commands::agents::InstalledFileText, ServiceError> {
        let inspector = self.handle(&self.modules.inspector)?;
        let packages = self.handle(&self.modules.packages)?;
        let id = self.restate(input.agent_instance_id)?;
        let agent = packages.get(id).await.map_err(|e| self.module_error(e))?;
        let folder = std::path::PathBuf::from(agent.instance.agent_folder_path);
        let file = inspector
            .installed_file(id, &folder, &input.path)
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(file)
    }

    /// `agents.part_page`: `part_page(agent_instance_id, part_id, bundle_dir, detail)` with the
    /// directory from the agent's bundle.
    // spec: ordin-desktop/components/ServiceApi#SA-CLOSE-06
    pub(super) async fn agents_part_page(
        &self,
        input: PartPageInput,
    ) -> Result<ordin_service_api::commands::agents::PartPage, ServiceError> {
        let inspector = self.handle(&self.modules.inspector)?;
        let id = self.restate(input.agent_instance_id)?;
        let bundle = self.bundle_of(id).await?;
        let detail = self.restate(input.detail)?;
        let page = inspector
            .part_page(id, &input.part_id, &bundle.dir, detail)
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(page)
    }

    /// `checker.check_installed`: `check_installed(id, lock, scope)` with the lock file from the
    /// agent's bundle and the scope from `PackageCatalogue::source_scope(id)`.
    // spec: ordin-desktop/components/ServiceApi#SA-CLOSE-01
    pub(super) async fn checker_check_installed(
        &self,
        input: CheckInstalledInput,
    ) -> Result<ordin_service_api::commands::checker::CheckReport, ServiceError> {
        let checker = self.handle(&self.modules.checker)?;
        let packages = self.handle(&self.modules.packages)?;
        let id = self.restate(input.agent_instance_id)?;
        let bundle = self.bundle_of(id).await?;
        let scope = packages
            .source_scope(id)
            .await
            .map_err(|e| self.module_error(e))?;
        let report = checker
            .check_installed(id, &bundle.lock_file, &scope)
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(report)
    }

    /// `checker.check_library_item`: `check_library_item(scope, kind, identifier)` with the scope
    /// `SourceScope { home, profile, include_bundled: true }` of `CatalogueService::source_scope`;
    /// no repository means the bundled catalogue.
    // spec: ordin-desktop/components/ServiceApi#SA-CLOSE-01
    pub(super) async fn checker_check_library_item(
        &self,
        input: CheckLibraryItemInput,
    ) -> Result<ordin_service_api::commands::checker::CheckReport, ServiceError> {
        let checker = self.handle(&self.modules.checker)?;
        let catalogue = self.handle(&self.modules.catalogue)?;
        let home = match input.agent_repository_id {
            Some(id) => RepositoryRef::Repository(self.restate(id)?),
            None => RepositoryRef::Bundled,
        };
        let scope = catalogue
            .source_scope(home)
            .await
            .map_err(|e| self.module_error(e))?;
        let kind = self.restate(input.kind)?;
        let report = checker
            .check_library_item(&scope, kind, &input.identifier)
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(report)
    }

    /// The `CredentialView` of a saved credential: the row of `IdentityService::list()` with the same
    /// id (IdentityAndCredentials.md ID-R1-2).
    async fn view_of(
        &self,
        credential: &ordin_types::domain::identity::Credential,
    ) -> Result<CredentialView, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let rows = identity.list().await.map_err(|e| self.module_error(e))?;
        let row = rows
            .into_iter()
            .find(|row| row.id == credential.id)
            .ok_or_else(|| self.refuse(super::INTERNAL, None))?;
        self.restate(row)
    }

    /// `identity.sign_in`: the provider and scopes of the kind (ID-R1-1), the Browser method.
    // spec: ordin-desktop/components/IdentityAndCredentials#ID-R1-1
    // spec: ordin-desktop/components/IdentityAndCredentials#ID-R1-3
    pub(super) async fn identity_sign_in(
        &self,
        input: IdentitySignInInput,
    ) -> Result<SignInChallenge, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let profile = self.handle(&self.modules.profile)?;
        let document = profile.current().document.clone();
        let (provider, scopes) = sign_in_provider(
            &document,
            input.kind,
            input.site.as_deref(),
            input.mail_account.as_deref(),
        )
        .map_err(|code| self.own_refusal(code, false))?;
        let challenge = identity
            .begin_sign_in(SignInRequest {
                provider,
                scopes,
                method: SignInMethod::Browser,
                relay_port: None,
                reason: input.reason,
            })
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(challenge)
    }

    /// `identity.sign_in_wait`: `wait_for_sign_in`, answered as the credential's view (ID-R1-2).
    pub(super) async fn identity_sign_in_wait(
        &self,
        input: IdentitySignInWaitInput,
    ) -> Result<CredentialView, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let challenge = self.restate(input.challenge_id)?;
        let credential = identity
            .wait_for_sign_in(challenge)
            .await
            .map_err(|e| self.module_error(e))?;
        self.view_of(&credential).await
    }

    /// `identity.save_mail_password`: the password goes in as a secret and is never logged.
    pub(super) async fn identity_save_mail_password(
        &self,
        input: IdentitySaveMailPasswordInput,
    ) -> Result<CredentialView, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let kind = self.restate(input.kind)?;
        let credential = identity
            .save_mail_password(
                &input.account_label,
                kind,
                SecretString::new(input.password),
            )
            .await
            .map_err(|e| self.module_error(e))?;
        self.view_of(&credential).await
    }

    /// `identity.save_gitlab_token`: the host from the profile; a profile with a GitLab OAuth
    /// application offers no pasted token (ID-R1-2).
    pub(super) async fn identity_save_gitlab_token(
        &self,
        input: IdentitySaveGitlabTokenInput,
    ) -> Result<CredentialView, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let profile = self.handle(&self.modules.profile)?;
        let document = profile.current().document.clone();
        let host = match document.gitlab.as_ref() {
            Some(section) if section.client_id.is_some() => {
                return Err(self.own_refusal(FLOW_NOT_OFFERED, false));
            }
            Some(section) => section.host.clone(),
            None => "gitlab.com".to_owned(),
        };
        let credential = identity
            .save_gitlab_token(&host, SecretString::new(input.token))
            .await
            .map_err(|e| self.module_error(e))?;
        self.view_of(&credential).await
    }

    /// `identity.save_github_token`: a personal access token, checked by Identity against GitHub; a
    /// build with the GitHub App offers the device sign-in instead (`CRED_FLOW_NOT_OFFERED`).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-GH-PAT-1
    pub(super) async fn identity_save_github_token(
        &self,
        input: IdentitySaveGithubTokenInput,
    ) -> Result<CredentialView, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let credential = identity
            .save_github_token(SecretString::new(input.token))
            .await
            .map_err(|e| self.module_error(e))?;
        self.view_of(&credential).await
    }

    /// `identity.import_client_certificate`: two picks, the certificate then the key (DSK-R1-1).
    // spec: ordin-desktop/components/OrdinDesktop#DSK-R1-1
    pub(super) async fn identity_import_client_certificate(
        &self,
    ) -> Result<IdentityImportClientCertificateResult, ServiceError> {
        let identity = self.handle(&self.modules.identity)?;
        let profile = self.handle(&self.modules.profile)?;
        let shell = self.handle(&self.modules.shell)?;
        let name = profile
            .current()
            .document
            .server
            .client_certificate_name
            .clone()
            .ok_or_else(|| self.own_refusal(NOT_CONFIGURED, false))?;
        let picked =
            |_: crate::shell::PickerUnavailable| self.own_refusal(PICKER_UNAVAILABLE, true);
        let Some(certificate_path) = shell
            .pick_file("Choose the certificate file")
            .await
            .map_err(picked)?
        else {
            return Ok(IdentityImportClientCertificateResult { certificate: None });
        };
        let Some(key_path) = shell
            .pick_file("Choose the key file")
            .await
            .map_err(picked)?
        else {
            return Ok(IdentityImportClientCertificateResult { certificate: None });
        };
        let read = |path: &std::path::Path| -> Result<Vec<u8>, ServiceError> {
            let bytes =
                std::fs::read(path).map_err(|_| self.own_refusal(CERTIFICATE_INVALID, false))?;
            if bytes.len() > CERTIFICATE_FILE_LIMIT {
                return Err(self.own_refusal(CERTIFICATE_INVALID, false));
            }
            Ok(bytes)
        };
        let certificate = read(&certificate_path)?;
        let key = Zeroizing::new(read(&key_path)?);
        let view = identity
            .import_client_certificate(&name, &certificate, key)
            .await
            .map_err(|e| self.module_error(e))?;
        Ok(IdentityImportClientCertificateResult {
            certificate: Some(self.restate(view)?),
        })
    }

    /// `confluence.publish_checked`: the link and its place are read first (WS-R1-1).
    // spec: ordin-desktop/components/ServiceApi#SVC-R1-4
    pub(super) async fn confluence_publish_checked(
        &self,
        input: ConfluencePublishCheckedInput,
    ) -> Result<ConfluencePublishCheckedResult, ServiceError> {
        let confluence = self.handle(&self.modules.confluence)?;
        let workspaces = self.handle(&self.modules.workspaces)?;
        let files = self.handle(&self.modules.files)?;
        let link = workspaces
            .confluence_link(self.restate(input.confluence_link_id)?)
            .await
            .map_err(|e| self.module_error(e))?;
        let place = files
            .place(link.place_id)
            .await
            .map_err(|e| self.module_error(e))?;
        let items = self.restate(input.items)?;
        let approval_id = confluence
            .request_publish_checked(&link, &place, items)
            .await
            .map_err(|e| self.module_error(e))?;
        Ok(ConfluencePublishCheckedResult {
            approval_id: self.restate(approval_id)?,
        })
    }

    /// `files.save_edit`: a person's edit (FS-R1-1).
    pub(super) async fn files_save_edit(
        &self,
        input: FilesSaveEditInput,
    ) -> Result<ordin_service_api::commands::files::FileVersion, ServiceError> {
        let files = self.handle(&self.modules.files)?;
        let version = files
            .write_person_edit(
                self.restate(input.place_id)?,
                &input.path,
                input.text.into_bytes(),
            )
            .await
            .map_err(|e| self.module_error(e))?;
        self.restate(version)
    }
}
