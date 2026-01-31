//! Connecting Confluence with a pasted API token (ConfluenceAdapter.md CONF-TOKEN-1; amended January
//! 2026; user decision; Approved by the user January 2026): the site, the email and the token are checked
//! with one cheap authenticated read, `GET /wiki/rest/api/user/current`, before Identity keeps the token.
//! The token is never logged, never journalled and never part of a reply.

use ordin_identity::SecretString;
use serde_json::Value;

use crate::error::ConfluenceError;
use crate::types::ConfluenceUser;
use crate::ConfluenceAdapter;

/// Lower case `{name}.atlassian.net`; `name` is 1–63 letters, digits or `-`, starting with a letter or
/// a digit (the Jira Adapter's site rule).
pub(crate) fn site_ok(host: &str) -> bool {
    let Some(name) = host.strip_suffix(".atlassian.net") else {
        return false;
    };
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first_ok = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    first_ok
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// What the check of a pasted token came to.
enum Checked {
    Person(ConfluenceUser),
    Refused,
    WrongSite,
    Unavailable,
}

impl ConfluenceAdapter {
    /// CONF-TOKEN-1: checks `email` and `token` on `site` (`{name}.atlassian.net`) with
    /// `GET https://{site}/wiki/rest/api/user/current`, then keeps them through Identity as the person's
    /// Confluence credential. Every later Confluence request then uses them (CONF-TOKEN-2).
    ///
    /// | Outcome | Error |
    /// |---|---|
    /// | site not `{name}.atlassian.net` | `CONF_SITE_INVALID` |
    /// | email not 3–254 characters with one `@`, token not 1–1,024 bytes | `CONF_TOKEN_REFUSED` |
    /// | 401, 403, or the anonymous user | `CONF_TOKEN_REFUSED` |
    /// | 404, or a reply that is not a Confluence user | `CONF_SITE_INVALID` |
    /// | no answer, 429 or 5xx | `CONF_UNAVAILABLE` |
    /// | Identity cannot keep the token | `CONF_UNAVAILABLE` (retryable) or `CONF_SIGN_IN_NEEDED` |
    // spec: ordin-desktop/components/ConfluenceAdapter#CONF-TOKEN-1
    pub async fn connect_with_token(
        &self,
        site: &str,
        email: &str,
        token: SecretString,
    ) -> Result<ConfluenceUser, ConfluenceError> {
        let site = site.trim().to_ascii_lowercase();
        if !site_ok(&site) {
            return Err(ConfluenceError::SiteInvalid);
        }
        let email = email.trim();
        let email_chars = email.chars().count();
        let token_ok = (1..=1024).contains(&token.expose().len());
        if !(3..=254).contains(&email_chars) || email.matches('@').count() != 1 || !token_ok {
            return Err(ConfluenceError::TokenRefused);
        }
        let person = match self.check_token(&site, email, &token).await {
            Checked::Person(person) => person,
            Checked::Refused => {
                tracing::info!(
                    component = "confluence",
                    accepted = false,
                    "confluence_token_checked"
                );
                return Err(ConfluenceError::TokenRefused);
            }
            Checked::WrongSite => {
                tracing::info!(
                    component = "confluence",
                    accepted = false,
                    "confluence_token_checked"
                );
                return Err(ConfluenceError::SiteInvalid);
            }
            Checked::Unavailable => return Err(ConfluenceError::Unavailable),
        };
        tracing::info!(
            component = "confluence",
            accepted = true,
            "confluence_token_checked"
        );
        self.transport
            .identity()
            .save_confluence_token(
                &site,
                email,
                token,
                &person.account_id,
                &person.display_name,
            )
            .await
            .map_err(|e| {
                if ordin_types::CodedError::retryable(&e) {
                    ConfluenceError::Unavailable
                } else {
                    ConfluenceError::SignInNeeded
                }
            })?;
        Ok(person)
    }

    /// The one authenticated read. A redirect is not followed by the check's verdict: Atlassian answers
    /// this address directly, so anything else is not a Confluence site.
    async fn check_token(&self, site: &str, email: &str, token: &SecretString) -> Checked {
        let (http, timeout) = self.transport.check_client();
        let url = format!(
            "{}/wiki/rest/api/user/current",
            self.transport.site_base(site)
        );
        let sent = http
            .get(url)
            .timeout(timeout)
            .basic_auth(email, Some(token.expose()))
            .header("accept", "application/json")
            .send()
            .await;
        let Ok(response) = sent else {
            return Checked::Unavailable;
        };
        let status = response.status().as_u16();
        tracing::info!(
            component = "confluence",
            operation = "connect_token",
            status = u64::from(status),
            "confluence_request"
        );
        match status {
            200..=299 => {}
            401 | 403 => return Checked::Refused,
            429 | 500..=599 => return Checked::Unavailable,
            _ => return Checked::WrongSite,
        }
        let Ok(body) = response.bytes().await else {
            return Checked::Unavailable;
        };
        let Ok(me) = serde_json::from_slice::<Value>(&body) else {
            return Checked::WrongSite;
        };
        // Confluence answers an unauthenticated read with the anonymous user rather than 401.
        if me.get("type").and_then(Value::as_str) == Some("anonymous") {
            return Checked::Refused;
        }
        let text = |key: &str| {
            me.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        match (text("accountId"), text("displayName")) {
            (Some(account_id), Some(display_name))
                if account_id.chars().count() <= 128 && display_name.chars().count() <= 255 =>
            {
                Checked::Person(ConfluenceUser {
                    account_id,
                    display_name,
                })
            }
            _ => Checked::WrongSite,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::site_ok;

    #[test]
    fn only_an_atlassian_cloud_site_is_taken() {
        for good in [
            "acme.atlassian.net",
            "a1-b.atlassian.net",
            "9.atlassian.net",
        ] {
            assert!(site_ok(good), "{good}");
        }
        for bad in [
            "atlassian.net",
            ".atlassian.net",
            "-acme.atlassian.net",
            "Acme.atlassian.net",
            "acme.example.com",
            "https://acme.atlassian.net",
            "acme.atlassian.net/wiki",
            "a.b.atlassian.net",
        ] {
            assert!(!site_ok(bad), "{bad}");
        }
    }
}
