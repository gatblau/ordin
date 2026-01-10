//! Sign-ins — restated from `IdentityAndCredentials.md` (SA-RECON-02), with its amendments for Slack
//! (ADR-102: relayed browser sign-in) and Jira (ADR-106: the `site` input). Also the desktop's
//! `identity.relay_sign_in` of the program group (`OrdinDesktop.md`), whose result is this group's
//! `CredentialView`.
//!
//! The inputs that carry a secret (`password`, `token`, `code`) print `[redacted]` in `Debug`, so a
//! value never reaches a log line through a formatted input.

use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};

use super::rules;
use super::ServerRef;

pub use ordin_types::domain::identity::{Credential, CredentialKind};
use ordin_types::time::Timestamp;
pub use ordin_types::CredentialId;

/// Declares an identifier an owner types as a UUIDv7 newtype that `ordin-types` does not hold. The
/// serde form is the owner's (the 36-character lowercase hyphenated v7 UUID); the value is kept as that
/// text, checked with `ordin_types::id::parse_v7`, because this crate has no `uuid` dependency.
macro_rules! v7_text_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, schemars::JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Parses the 36-character form; refuses anything but a lowercase hyphenated v7 UUID.
            pub fn parse(raw: &str) -> Result<Self, ordin_types::TypesError> {
                ordin_types::id::parse_v7(raw).map(|_| Self(raw.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}
pub(crate) use v7_text_id;

v7_text_id!(
    /// A pending sign-in (UUIDv7, *chosen here* by the owner).
    SignInChallengeId
);

/// The bound of `relay_port`: the paired desktop's loopback ports (ADR-102).
const RELAY_PORT_MIN: u16 = 47821;
const RELAY_PORT_MAX: u16 = 47823;
/// `account_label` (2C, LD-10).
const ACCOUNT_LABEL_MAX_CHARS: usize = 254;
/// A mail password or app password: 1–1,024 bytes of UTF-8.
const PASSWORD_MAX_BYTES: usize = 1024;

// ---- results ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignInState {
    SignedOut,
    SignedIn,
    NeedsSignIn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CredentialView {
    pub id: CredentialId,
    pub kind: CredentialKind,
    pub account_label: String,
    pub state: SignInState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignInChallenge {
    /// The system browser was opened at the provider's page; wait for `identity.sign_in_wait`.
    BrowserOpened {
        challenge_id: SignInChallengeId,
        expires_at: Timestamp,
    },
    /// Show `verification_uri` and `user_code` to the person (chat or screen).
    DeviceCode {
        challenge_id: SignInChallengeId,
        verification_uri: String,
        user_code: String,
        expires_at: Timestamp,
    },
    /// An active credential already covers the requested scopes: nothing is asked.
    AlreadySignedIn { credential: Credential },
    /// Personal server: the paired desktop opens `authorisation_url` in the person's browser and
    /// forwards the code with `identity.complete_relayed_sign_in` (amended January 2026, ADR-102).
    RelayedBrowser {
        challenge_id: SignInChallengeId,
        authorisation_url: String,
        expires_at: Timestamp,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ClientCertificateView {
    pub name: String,
    /// The leaf certificate's subject common name, ≤ 254 characters.
    pub subject: String,
    pub not_after: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailPasswordKind {
    MailPassword,
    AppPassword,
}

/// `identity.list`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct IdentityListResult {
    pub items: Vec<CredentialView>,
}

/// `identity.import_client_certificate`: absent when the person cancelled either picker.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct IdentityImportClientCertificateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate: Option<ClientCertificateView>,
}

// ---- inputs ----

/// `identity.relay_sign_in` (OrdinDesktop.md, ADR-102): the desktop half of a server's relayed Slack
/// sign-in.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityRelaySignInInput {
    pub server: ServerRef,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityListInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentitySignInInput {
    pub kind: CredentialKind,
    pub reason: Option<String>,
    /// Personal server, `slack` only: the paired desktop's loopback port, 47821–47823 (ADR-102).
    pub relay_port: Option<u16>,
    /// `jira` only: the person's Jira site address (ADR-106).
    pub site: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentitySignInWaitInput {
    pub challenge_id: SignInChallengeId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentitySignInCancelInput {
    pub challenge_id: SignInChallengeId,
}

/// Carries a secret in (`code`); personal server only.
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityCompleteRelayedSignInInput {
    pub challenge_id: SignInChallengeId,
    pub code: String,
    pub state: String,
}

impl std::fmt::Debug for IdentityCompleteRelayedSignInInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityCompleteRelayedSignInInput")
            .field("challenge_id", &self.challenge_id)
            .field("code", &"[redacted]")
            .field("state", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentitySignOutInput {
    pub credential_id: CredentialId,
}

/// Carries a secret in (`password`).
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentitySaveMailPasswordInput {
    pub account_label: String,
    pub kind: MailPasswordKind,
    pub password: String,
}

impl std::fmt::Debug for IdentitySaveMailPasswordInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentitySaveMailPasswordInput")
            .field("account_label", &self.account_label)
            .field("kind", &self.kind)
            .field("password", &"[redacted]")
            .finish()
    }
}

/// Carries a secret in (`token`); GitLab without an OAuth application (SA-CLOSE-04).
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentitySaveGitlabTokenInput {
    pub token: String,
}

impl std::fmt::Debug for IdentitySaveGitlabTokenInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentitySaveGitlabTokenInput")
            .field("token", &"[redacted]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct IdentityImportClientCertificateInput {}

// ---- checks ----

impl CommandInput for IdentityRelaySignInInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentityListInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentitySignInInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::opt_range(
            self.relay_port,
            RELAY_PORT_MIN,
            RELAY_PORT_MAX,
            "/relay_port",
        )
    }
}

impl CommandInput for IdentitySignInWaitInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentitySignInCancelInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentityCompleteRelayedSignInInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentitySignOutInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentitySaveMailPasswordInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(
            &self.account_label,
            1,
            ACCOUNT_LABEL_MAX_CHARS,
            "/account_label",
        )?;
        rules::bytes(&self.password, 1, PASSWORD_MAX_BYTES, "/password")
    }
}

impl CommandInput for IdentitySaveGitlabTokenInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for IdentityImportClientCertificateInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHALLENGE: &str = "0192a1d0-2222-7333-8444-555566667777";

    #[test]
    fn challenge_ids() {
        let id = SignInChallengeId::parse(CHALLENGE).unwrap();
        assert_eq!(id.as_str(), CHALLENGE);
        assert_eq!(id.to_string(), CHALLENGE);
        assert!(SignInChallengeId::parse("0192A1D0-2222-7333-8444-555566667777").is_err());
        assert!(serde_json::from_str::<SignInChallengeId>("\"abc\"").is_err());
        let back: SignInChallengeId = serde_json::from_str(&format!("\"{CHALLENGE}\"")).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn sign_in_checks() {
        let input = |relay_port| IdentitySignInInput {
            kind: CredentialKind::Slack,
            reason: None,
            relay_port,
            site: None,
        };
        for (port, ok) in [
            (None, true),
            (Some(47821), true),
            (Some(47823), true),
            (Some(47820), false),
            (Some(47824), false),
        ] {
            let result = input(port).check();
            assert_eq!(result.is_ok(), ok, "{port:?}");
            if let Err(rejection) = result {
                assert_eq!(rejection.code, "SVC_INPUT_INVALID");
                assert_eq!(rejection.at, "/input/relay_port");
            }
        }
        let parsed: IdentitySignInInput =
            serde_json::from_str(r#"{"kind":"jira","site":"https://acme.atlassian.net"}"#).unwrap();
        assert_eq!(parsed.kind, CredentialKind::Jira);
        assert!(serde_json::from_str::<IdentitySignInInput>(r#"{"kind":"jira","x":1}"#).is_err());
    }

    #[test]
    fn mail_password_checks() {
        let input = |label: &str, password: &str| IdentitySaveMailPasswordInput {
            account_label: label.into(),
            kind: MailPasswordKind::AppPassword,
            password: password.into(),
        };
        assert!(input("asha@acme.com", "pw").check().is_ok());
        for (label, password, at) in [
            ("", "pw", "/input/account_label"),
            (&"a".repeat(255) as &str, "pw", "/input/account_label"),
            ("asha@acme.com", "", "/input/password"),
            ("asha@acme.com", &"p".repeat(1025), "/input/password"),
        ] {
            let rejection = input(label, password).check().unwrap_err();
            assert_eq!(
                (rejection.code, rejection.at.as_str()),
                ("SVC_INPUT_INVALID", at)
            );
        }
        let shown = format!("{:?}", input("asha@acme.com", "hunter2"));
        assert!(!shown.contains("hunter2"));
    }

    #[test]
    fn secrets_never_print_and_trivial_inputs_pass() {
        let token = IdentitySaveGitlabTokenInput {
            token: "glpat-secret".into(),
        };
        assert!(!format!("{token:?}").contains("glpat"));
        let mut relayed = IdentityCompleteRelayedSignInInput {
            challenge_id: SignInChallengeId::parse(CHALLENGE).unwrap(),
            code: "the-code".into(),
            state: "the-state".into(),
        };
        let shown = format!("{relayed:?}");
        assert!(!shown.contains("the-code") && !shown.contains("the-state"));
        assert!(relayed.check().is_ok());
        let mut token = token;
        assert!(token.check().is_ok());
        assert!(IdentityListInput {}.check().is_ok());
        assert!(IdentityImportClientCertificateInput {}.check().is_ok());
        let challenge_id = SignInChallengeId::parse(CHALLENGE).unwrap();
        assert!(IdentitySignInWaitInput {
            challenge_id: challenge_id.clone()
        }
        .check()
        .is_ok());
        assert!(IdentitySignInCancelInput { challenge_id }.check().is_ok());
        assert!(IdentitySignOutInput {
            credential_id: CredentialId::new()
        }
        .check()
        .is_ok());
        let mut relay: IdentityRelaySignInInput = serde_json::from_value(serde_json::json!({
            "server": {"server_connection_id": "0192a1d0-2222-7333-8444-555566667777"}
        }))
        .unwrap();
        assert!(relay.check().is_ok());
    }

    #[test]
    fn challenge_and_view_shapes() {
        let challenge = SignInChallenge::RelayedBrowser {
            challenge_id: SignInChallengeId::parse(CHALLENGE).unwrap(),
            authorisation_url: "https://slack.com/oauth/v2/authorize?x=1".into(),
            expires_at: Timestamp::from_unix_millis(0).unwrap(),
        };
        let json = serde_json::to_value(&challenge).unwrap();
        assert_eq!(json["kind"], "relayed_browser");
        let view = CredentialView {
            id: CredentialId::new(),
            kind: CredentialKind::Github,
            account_label: "asha".into(),
            state: SignInState::NeedsSignIn,
            expires_at: None,
        };
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["state"], "needs_sign_in");
        assert!(json.get("expires_at").is_none());
    }
}
