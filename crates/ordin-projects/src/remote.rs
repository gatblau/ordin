//! Credentials and remotes (Internal Logic 3).

use std::path::Path;

use ordin_identity::CredError;
use ordin_types::domain::CredentialKind;
use ordin_types::CodedError;

use crate::error::ProjectError;
use crate::git::{self, NetAuth};
use crate::host_cli::HostKind;
use crate::service::ProjectService;

/// The host of an `https://` URL, lower-case, without user, port or path. The authority ends at the
/// first `/`, `?` or `#`, as it does for the transport: in `https://evil.example#@github.com/x` the
/// host is `evil.example`. Anything a parser could read two ways is no host at all: more than one
/// `@`, a backslash, a space or control character, or a port that is not a number.
pub(crate) fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    if authority
        .chars()
        .any(|c| c == '\\' || c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let host_port = match authority.rsplit_once('@') {
        Some((user, _)) if user.contains('@') => return None,
        Some((_, host_port)) => host_port,
        None => authority,
    };
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    };
    let plain_host = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    let plain_port = port.is_none_or(|port| port.bytes().all(|b| b.is_ascii_digit()));
    (plain_host && plain_port).then(|| host.to_ascii_lowercase())
}

/// Internal Logic 3.1: `github.com` → GitHub; the company's GitLab host or `gitlab.com` → GitLab;
/// anything else, or a remote that is not HTTPS, is unreachable.
// spec: ordin-desktop/components/ProjectService#SA-CLOSE-01
pub(crate) fn remote_kind(url: &str, gitlab_host: &str) -> Result<HostKind, ProjectError> {
    let Some(host) = https_host(url) else {
        return Err(ProjectError::RemoteUnreachable);
    };
    if host == "github.com" {
        Ok(HostKind::Github)
    } else if host == gitlab_host.to_ascii_lowercase() || host == "gitlab.com" {
        Ok(HostKind::Gitlab)
    } else {
        crate::log::unsupported_host();
        Err(ProjectError::RemoteUnreachable)
    }
}

/// `CRED_SIGN_IN_REQUIRED` → sign-in needed; any other failure by its class (Internal Logic 3.2).
fn cred_failed(error: CredError) -> ProjectError {
    if error.code() == "CRED_SIGN_IN_REQUIRED" {
        ProjectError::SignInNeeded
    } else if error.retryable() {
        ProjectError::RemoteUnreachable
    } else {
        ProjectError::GitFailed
    }
}

impl ProjectService {
    /// The test's stand-in host when `origin` is under its address (PROJ-SEAM-2).
    #[cfg(any(test, feature = "test-support"))]
    fn test_host_of(&self, url: &str) -> Option<crate::service::TestHost> {
        crate::service::lock(&self.tunables)
            .test_host
            .clone()
            .filter(|host| url.starts_with(&host.address))
    }

    /// The kind and the host of the repository's `origin`.
    async fn origin_host(&self, folder: &Path) -> Result<(HostKind, String), ProjectError> {
        let url = self.origin_url(folder).await?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(host) = self.test_host_of(&url) {
            return Ok((HostKind::Github, host.address));
        }
        // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-1
        let kind = remote_kind(&url, &self.current_settings().gitlab_host)?;
        let host = https_host(&url).ok_or(ProjectError::RemoteUnreachable)?;
        Ok((kind, host))
    }

    /// `origin`'s configured URL.
    async fn origin_url(&self, folder: &Path) -> Result<String, ProjectError> {
        let probe = folder.to_path_buf();
        git::blocking(move || git::origin_url(&probe))
            .await?
            .ok_or(ProjectError::RemoteUnreachable)
    }

    /// The kind of the repository's `origin`.
    pub(crate) async fn origin_kind(&self, folder: &Path) -> Result<HostKind, ProjectError> {
        self.origin_host(folder).await.map(|(kind, _)| kind)
    }

    /// The token for one network operation, taken from Identity and dropped after it, and the host
    /// it may go to.
    pub(crate) async fn remote_auth(&self, folder: &Path) -> Result<NetAuth, ProjectError> {
        // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-2
        #[cfg(any(test, feature = "test-support"))]
        if let Some(host) = self.test_host_of(&self.origin_url(folder).await?) {
            return Ok(NetAuth {
                username: HostKind::Github.git_username(),
                token: host.token,
                host: host.address.clone(),
                local_remote: None,
                test_address: Some(host.address),
            });
        }
        // spec: ordin-desktop/components/ProjectService#PROJ-SEAM-1
        let (kind, host) = self.origin_host(folder).await?;
        let credential = match kind {
            HostKind::Github => CredentialKind::Github,
            HostKind::Gitlab => CredentialKind::Gitlab,
        };
        let token = self
            .identity
            .access_token(credential, None)
            .await
            .map_err(cred_failed)?;
        Ok(NetAuth {
            username: kind.git_username(),
            token: token.token.expose().to_owned(),
            host,
            #[cfg(any(test, feature = "test-support"))]
            local_remote: crate::service::lock(&self.tunables).local_remote.clone(),
            #[cfg(any(test, feature = "test-support"))]
            test_address: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_host_reads_the_host_the_transport_would_reach() {
        let cases = [
            ("https://github.com/acme/x.git", Some("github.com")),
            ("https://GitHub.com/acme/x.git", Some("github.com")),
            ("https://github.com", Some("github.com")),
            (
                "https://x-access-token:t@github.com:443/a",
                Some("github.com"),
            ),
            ("https://github.com?a=b", Some("github.com")),
            ("https://evil.example#@github.com/x", Some("evil.example")),
            ("https://evil.example?@github.com/x", Some("evil.example")),
            ("https://evil.example/@github.com/x", Some("evil.example")),
            ("https://github.com@evil.example/x", Some("evil.example")),
            (
                "https://github.com:443@evil.example/x",
                Some("evil.example"),
            ),
            ("https://evil.example\\@github.com/x", None),
            ("https://a@evil.example@github.com/x", None),
            ("https://github.com:evil/x", None),
            ("https://evil example@github.com/x", None),
            ("https://[::1]/x", None),
            ("https://", None),
            ("https://@/x", None),
            ("http://github.com/x", None),
            ("git@github.com:acme/x.git", None),
            ("file:///tmp/github.com", None),
        ];
        for (url, host) in cases {
            assert_eq!(https_host(url).as_deref(), host, "{url}");
        }
    }

    #[test]
    fn a_url_that_only_names_github_after_its_authority_is_not_github() {
        let error = remote_kind("https://evil.example#@github.com/x", "gitlab.acme.co.uk");
        assert_eq!(error.unwrap_err().code(), "PROJ_REMOTE_UNREACHABLE");
    }
}
