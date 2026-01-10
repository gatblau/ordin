//! Workspaces — restated from `WorkspaceService.md` (SA-RECON-02).
//!
//! Workspaces, their links, grants, Confluence links, email links and chat grants. The entities are 2C
//! types and are imported; the service's own inputs and results are restated here. `workspaces.tree`
//! takes and returns this spec's root types (`WorkspaceRef`, `WorkspaceTree`).

use ordin_types::prelude::*;
use serde::{Deserialize, Serialize};

use super::confluence::ConfluenceLocation;
use super::rules;
use super::sharepoint::SharePointLocation;
use crate::api::{CommandInput, InputRejection};

pub use ordin_types::domain::{
    ConfluenceLink, EmailAccess, EmailGrant, EmailLink, EmailLinkKind, FolderAccess, FolderGrant,
    Grantee, SlackAccess, SlackGrant, SourceLink, TelegramAccess, TelegramGrant, Workspace,
};

/// `Workspace.name`: 1–80 characters after trimming (2C, LD-24).
const WORKSPACE_NAME_MAX_CHARS: usize = 80;
/// `SourceLink.display_name`: 1–120 characters.
const LINK_NAME_MAX_CHARS: usize = 120;
/// `SourceLink.origin_location`: 1–2,048 bytes.
const ORIGIN_LOCATION_MAX_BYTES: usize = 2048;
/// A local path: at most 4,096 bytes (2C shared path rule).
const LOCAL_PATH_MAX_BYTES: usize = 4096;
/// `LinkLocation::GitRemote.branch`: 1–255 characters.
const BRANCH_MAX_CHARS: usize = 255;
/// `LinkLocation::GitRemote.path`: at most 1,024 bytes.
const GIT_PATH_MAX_BYTES: usize = 1024;
/// `EmailLink.folder_or_label`: 1–255 characters.
const FOLDER_OR_LABEL_MAX_CHARS: usize = 255;
/// `EmailLink.addresses`: at most 200, each at most 254 characters.
const MAX_ADDRESSES: usize = 200;
const ADDRESS_MAX_CHARS: usize = 254;
/// `EmailLink.domains`: at most 100.
const MAX_DOMAINS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceListFilter {
    Active,
    Archived,
    InBin,
    NotInBin,
}

/// Where a new link points.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkLocation {
    LocalFolder {
        path: String,
    },
    GitRepositoryFolder {
        path: String,
    },
    Sharepoint {
        location: SharePointLocation,
    },
    Confluence {
        location: ConfluenceLocation,
    },
    /// A folder of a GitHub or GitLab repository, linked without a local clone (SA-CLOSE-03). `url`
    /// `https://<host>/<owner>/<name>(.git)?` or `git@<host>:<owner>/<name>(.git)?`; `branch` 1–255
    /// characters; `path` relative to the repository root, "" for the root, ≤ 1,024 bytes, no `..`.
    GitRemote {
        url: String,
        branch: String,
        path: String,
    },
    /// A Jira Cloud issue address as the person pasted or picked it (amended January 2026, ADR-106).
    JiraIssue {
        address: String,
    },
}

/// How far the first read of a remote link has got.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LinkProgress {
    pub source_link_id: SourceLinkId,
    pub listed: u64,
    pub read: u64,
    /// Beyond `LINK_READ_BUDGET` or failed.
    pub not_read: u64,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct OpenReport {
    pub workspace: Workspace,
    /// Changes found by the adapters at this open.
    pub origin_changes: u32,
    pub checks_failed: Vec<SourceLinkId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct BinImpact {
    /// Documents changed here and not yet sent back (working-copy items not `read`/`pushed`, and held edits).
    pub unsent_changes: u32,
    /// Display names, at most 10.
    pub documents: Vec<String>,
}

// ---- inputs ----

/// Declares the inputs whose only field is one identifier and that carry no rule beyond its type.
macro_rules! id_inputs {
    ($($(#[$doc:meta])* $name:ident { $field:ident: $ty:ty });* $(;)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "snake_case", deny_unknown_fields)]
        pub struct $name {
            pub $field: $ty,
        }

        impl CommandInput for $name {
            fn check(&mut self) -> Result<(), InputRejection> {
                Ok(())
            }
        }
    )*};
}

id_inputs! {
    /// `workspaces.archive`.
    WorkspacesArchiveInput { workspace_id: WorkspaceId };
    /// `workspaces.restore`.
    WorkspacesRestoreInput { workspace_id: WorkspaceId };
    /// `workspaces.bin_impact`.
    WorkspacesBinImpactInput { workspace_id: WorkspaceId };
    /// `workspaces.move_to_bin`.
    WorkspacesMoveToBinInput { workspace_id: WorkspaceId };
    /// `workspaces.restore_from_bin`.
    WorkspacesRestoreFromBinInput { workspace_id: WorkspaceId };
    /// `workspaces.delete_from_bin`.
    WorkspacesDeleteFromBinInput { workspace_id: WorkspaceId };
    /// `workspaces.list`.
    WorkspacesListInput { filter: WorkspaceListFilter };
    /// `workspaces.open`.
    WorkspacesOpenInput { workspace_id: WorkspaceId };
    /// `workspaces.links`.
    WorkspacesLinksInput { workspace_id: WorkspaceId };
    /// `workspaces.confirm_link`.
    WorkspacesConfirmLinkInput { source_link_id: SourceLinkId };
    /// `workspaces.decline_link`.
    WorkspacesDeclineLinkInput { source_link_id: SourceLinkId };
    /// `workspaces.remove_link`.
    WorkspacesRemoveLinkInput { source_link_id: SourceLinkId };
    /// `workspaces.link_progress`.
    WorkspacesLinkProgressInput { source_link_id: SourceLinkId };
    /// `workspaces.grants`.
    WorkspacesGrantsInput { workspace_id: WorkspaceId };
    /// `workspaces.pause_confluence_link`.
    WorkspacesPauseConfluenceLinkInput { confluence_link_id: ConfluenceLinkId };
    /// `workspaces.resume_confluence_link`.
    WorkspacesResumeConfluenceLinkInput { confluence_link_id: ConfluenceLinkId };
    /// `workspaces.remove_confluence_link`.
    WorkspacesRemoveConfluenceLinkInput { confluence_link_id: ConfluenceLinkId };
    /// `workspaces.email_links`.
    WorkspacesEmailLinksInput { workspace_id: WorkspaceId };
    /// `workspaces.remove_email_link`.
    WorkspacesRemoveEmailLinkInput { email_link_id: EmailLinkId };
    /// `workspaces.telegram_grants`.
    WorkspacesTelegramGrantsInput { workspace_id: WorkspaceId };
    /// `workspaces.slack_grants`.
    WorkspacesSlackGrantsInput { workspace_id: WorkspaceId };
}

/// `workspaces.create`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesCreateInput {
    pub name: String,
}

/// `workspaces.rename`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesRenameInput {
    pub workspace_id: WorkspaceId,
    pub name: String,
}

/// `workspaces.empty_bin`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesEmptyBinInput {}

/// `workspaces.empty_bin`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkspacesEmptyBinResult {
    pub deleted: u32,
}

/// `workspaces.add_link`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesAddLinkInput {
    pub workspace_id: WorkspaceId,
    pub location: LinkLocation,
    pub display_name: Option<String>,
}

/// `workspaces.set_grant`; `access` absent is "can't use": the grant is removed.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesSetGrantInput {
    pub workspace_id: WorkspaceId,
    pub grantee: Grantee,
    pub place_id: PlaceId,
    pub access: Option<FolderAccess>,
}

/// `workspaces.propose_confluence_link`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesProposeConfluenceLinkInput {
    pub workspace_id: WorkspaceId,
    pub place_id: PlaceId,
    pub folder: String,
    pub location: ConfluenceLocation,
}

/// `workspaces.add_email_link`: proposed, then confirmed.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesAddEmailLinkInput {
    pub workspace_id: WorkspaceId,
    pub kind: EmailLinkKind,
    pub folder_or_label: Option<String>,
    pub addresses: Vec<String>,
    pub domains: Vec<String>,
    pub display_name: String,
}

/// `workspaces.set_email_grant`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesSetEmailGrantInput {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: Option<EmailAccess>,
}

/// `workspaces.set_telegram_grant`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesSetTelegramGrantInput {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: Option<TelegramAccess>,
}

/// `workspaces.set_slack_grant` (ADR-102).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspacesSetSlackGrantInput {
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub access: Option<SlackAccess>,
}

/// `Workspace.name`: 1–80 characters after trimming.
fn workspace_name(name: &str) -> Result<(), InputRejection> {
    rules::text(name.trim(), 1, WORKSPACE_NAME_MAX_CHARS, "/name")
}

impl CommandInput for WorkspacesCreateInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        workspace_name(&self.name)
    }
}

impl CommandInput for WorkspacesRenameInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        workspace_name(&self.name)
    }
}

impl CommandInput for WorkspacesEmptyBinInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

/// True for `https://<host>/<owner>/<name>(.git)?` or `git@<host>:<owner>/<name>(.git)?`.
fn is_git_remote_url(url: &str) -> bool {
    let (host, rest) = if let Some(rest) = url.strip_prefix("https://") {
        match rest.split_once('/') {
            Some(parts) => parts,
            None => return false,
        }
    } else if let Some(rest) = url.strip_prefix("git@") {
        match rest.split_once(':') {
            Some(parts) => parts,
            None => return false,
        }
    } else {
        return false;
    };
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut segments = rest.split('/');
    let segment_ok = |s: Option<&str>| {
        s.is_some_and(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    };
    rules::is_host(host)
        && segment_ok(segments.next())
        && segment_ok(segments.next())
        && segments.next().is_none()
}

/// The forms of a link location's fields (WorkspaceService.md `LinkLocation`, 2C `SourceLink`).
fn check_location(location: &LinkLocation) -> Result<(), InputRejection> {
    match location {
        LinkLocation::LocalFolder { path } | LinkLocation::GitRepositoryFolder { path } => {
            rules::bytes(path, 1, LOCAL_PATH_MAX_BYTES, "/location/path")
        }
        // The adapters' location types carry their own forms, checked by the adapter.
        LinkLocation::Sharepoint { .. } | LinkLocation::Confluence { .. } => Ok(()),
        LinkLocation::GitRemote { url, branch, path } => {
            if !is_git_remote_url(url) {
                return Err(rules::reject("/location/url"));
            }
            rules::text(branch, 1, BRANCH_MAX_CHARS, "/location/branch")?;
            if !path.is_empty() {
                rules::relative_path(path, "/location/path")?;
            }
            rules::bytes(path, 0, GIT_PATH_MAX_BYTES, "/location/path")
        }
        // The canonical issue form is the Jira Adapter's (`WS_JIRA_ADDRESS_INVALID`); only the
        // stored origin location's length is checked here.
        LinkLocation::JiraIssue { address } => {
            rules::bytes(address, 1, ORIGIN_LOCATION_MAX_BYTES, "/location/address")
        }
    }
}

impl CommandInput for WorkspacesAddLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_location(&self.location)?;
        rules::opt_text(&self.display_name, 1, LINK_NAME_MAX_CHARS, "/display_name")
    }
}

impl CommandInput for WorkspacesSetGrantInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for WorkspacesProposeConfluenceLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for WorkspacesAddEmailLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // 2C `EmailLink`: a folder or label alone, or at least one address or domain and no folder.
        match self.kind {
            EmailLinkKind::FolderOrLabel => {
                let Some(folder) = &self.folder_or_label else {
                    return Err(rules::reject("/folder_or_label"));
                };
                rules::text(folder, 1, FOLDER_OR_LABEL_MAX_CHARS, "/folder_or_label")?;
                rules::count(&self.addresses, 0, 0, "/addresses")?;
                rules::count(&self.domains, 0, 0, "/domains")?;
            }
            EmailLinkKind::PeopleAndDomains => {
                if self.folder_or_label.is_some() {
                    return Err(rules::reject("/folder_or_label"));
                }
                if self.addresses.is_empty() && self.domains.is_empty() {
                    return Err(rules::reject("/addresses"));
                }
            }
        }
        rules::count(&self.addresses, 0, MAX_ADDRESSES, "/addresses")?;
        for (i, address) in self.addresses.iter().enumerate() {
            rules::email(address, 3, ADDRESS_MAX_CHARS, &format!("/addresses/{i}"))?;
        }
        rules::count(&self.domains, 0, MAX_DOMAINS, "/domains")?;
        for (i, domain) in self.domains.iter().enumerate() {
            rules::host(domain, &format!("/domains/{i}"))?;
        }
        Ok(())
    }
}

impl CommandInput for WorkspacesSetEmailGrantInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for WorkspacesSetTelegramGrantInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for WorkspacesSetSlackGrantInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused_at(result: Result<(), InputRejection>) -> String {
        let err = result.unwrap_err();
        assert_eq!(err.code, "SVC_INPUT_INVALID");
        err.at
    }

    #[test]
    fn workspace_names() {
        let mut ok = WorkspacesCreateInput {
            name: "  Q3 supplier review ".into(),
        };
        assert!(ok.check().is_ok());
        for bad in ["   ", &"x".repeat(81)] {
            let mut create = WorkspacesCreateInput { name: bad.into() };
            assert_eq!(refused_at(create.check()), "/input/name");
            let mut rename = WorkspacesRenameInput {
                workspace_id: WorkspaceId::new(),
                name: bad.into(),
            };
            assert_eq!(refused_at(rename.check()), "/input/name");
        }
    }

    #[test]
    fn git_remote_urls() {
        for good in [
            "https://github.com/acme/supplier-docs",
            "https://gitlab.example.com/acme/docs.git",
            "git@github.com:acme/supplier-docs.git",
        ] {
            assert!(is_git_remote_url(good), "{good}");
        }
        for bad in [
            "http://github.com/acme/docs",
            "https://github.com/acme",
            "https://github.com/acme/docs/extra",
            "git@github.com/acme/docs",
            "git@github.com:acme/..",
            "https://github.com",
            "ssh://github.com/acme/docs",
        ] {
            assert!(!is_git_remote_url(bad), "{bad}");
        }
    }

    #[test]
    fn link_locations() {
        let add = |location| WorkspacesAddLinkInput {
            workspace_id: WorkspaceId::new(),
            location,
            display_name: None,
        };
        let remote = |url: &str, branch: &str, path: &str| LinkLocation::GitRemote {
            url: url.into(),
            branch: branch.into(),
            path: path.into(),
        };
        let url = "https://github.com/acme/docs";
        assert!(add(remote(url, "main", "")).check().is_ok());
        assert!(add(remote(url, "main", "knowledge/team")).check().is_ok());
        let cases = [
            (remote("x", "main", ""), "/input/location/url"),
            (remote(url, "", ""), "/input/location/branch"),
            (remote(url, "main", "../x"), "/input/location/path"),
            (remote(url, "main", "/x"), "/input/location/path"),
            (
                LinkLocation::LocalFolder {
                    path: String::new(),
                },
                "/input/location/path",
            ),
            (
                LinkLocation::GitRepositoryFolder {
                    path: "x".repeat(4097),
                },
                "/input/location/path",
            ),
            (
                LinkLocation::JiraIssue {
                    address: String::new(),
                },
                "/input/location/address",
            ),
        ];
        for (location, at) in cases {
            assert_eq!(refused_at(add(location).check()), at);
        }
        assert!(add(LinkLocation::LocalFolder {
            path: "/Users/asha/Documents".into()
        })
        .check()
        .is_ok());
        assert!(add(LinkLocation::JiraIssue {
            address: "https://acme.atlassian.net/browse/ABC-12".into()
        })
        .check()
        .is_ok());
        let mut named = add(LinkLocation::LocalFolder { path: "/a".into() });
        named.display_name = Some(String::new());
        assert_eq!(refused_at(named.check()), "/input/display_name");
        let jira: LinkLocation = serde_json::from_value(
            serde_json::json!({"kind": "jira_issue", "address": "https://acme.atlassian.net/browse/ABC-12"}),
        )
        .unwrap();
        assert!(check_location(&jira).is_ok());
    }

    #[test]
    fn email_links() {
        let link = |kind, folder: Option<&str>, addresses: &[&str], domains: &[&str]| {
            WorkspacesAddEmailLinkInput {
                workspace_id: WorkspaceId::new(),
                kind,
                folder_or_label: folder.map(str::to_owned),
                addresses: addresses.iter().map(|s| s.to_string()).collect(),
                domains: domains.iter().map(|s| s.to_string()).collect(),
                display_name: "Kestrel".into(),
            }
        };
        use EmailLinkKind::{FolderOrLabel, PeopleAndDomains};
        assert!(link(FolderOrLabel, Some("Suppliers"), &[], &[])
            .check()
            .is_ok());
        assert!(link(
            PeopleAndDomains,
            None,
            &["sam@kestrel.example"],
            &["acme.example"]
        )
        .check()
        .is_ok());
        let many: Vec<String> = (0..201).map(|i| format!("p{i}@a.example")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let cases = [
            (
                link(FolderOrLabel, None, &[], &[]),
                "/input/folder_or_label",
            ),
            (
                link(FolderOrLabel, Some(""), &[], &[]),
                "/input/folder_or_label",
            ),
            (
                link(FolderOrLabel, Some("x"), &["a@b.c"], &[]),
                "/input/addresses",
            ),
            (
                link(FolderOrLabel, Some("x"), &[], &["a.example"]),
                "/input/domains",
            ),
            (
                link(PeopleAndDomains, Some("x"), &["a@b.c"], &[]),
                "/input/folder_or_label",
            ),
            (link(PeopleAndDomains, None, &[], &[]), "/input/addresses"),
            (
                link(PeopleAndDomains, None, &["nobody"], &[]),
                "/input/addresses/0",
            ),
            (
                link(PeopleAndDomains, None, &[], &["bad domain"]),
                "/input/domains/0",
            ),
            (link(PeopleAndDomains, None, &many, &[]), "/input/addresses"),
        ];
        for (mut input, at) in cases {
            assert_eq!(refused_at(input.check()), at);
        }
        let domains: Vec<String> = (0..101).map(|i| format!("d{i}.example")).collect();
        let domains: Vec<&str> = domains.iter().map(String::as_str).collect();
        assert_eq!(
            refused_at(link(PeopleAndDomains, None, &[], &domains).check()),
            "/input/domains"
        );
    }

    #[test]
    fn inputs_without_rules_pass() {
        let ws = WorkspaceId::new();
        let agent = AgentInstanceId::new();
        assert!(WorkspacesEmptyBinInput {}.check().is_ok());
        assert!(WorkspacesArchiveInput { workspace_id: ws }.check().is_ok());
        assert!(WorkspacesSetGrantInput {
            workspace_id: ws,
            grantee: Grantee::AgentInstance {
                agent_instance_id: agent
            },
            place_id: PlaceId::new(),
            access: None,
        }
        .check()
        .is_ok());
        assert!(WorkspacesSetEmailGrantInput {
            workspace_id: ws,
            agent_instance_id: agent,
            access: None
        }
        .check()
        .is_ok());
        assert!(WorkspacesSetTelegramGrantInput {
            workspace_id: ws,
            agent_instance_id: agent,
            access: None
        }
        .check()
        .is_ok());
        assert!(WorkspacesSetSlackGrantInput {
            workspace_id: ws,
            agent_instance_id: agent,
            access: None
        }
        .check()
        .is_ok());
        let refused = serde_json::from_value::<WorkspacesListInput>(
            serde_json::json!({"filter": "active", "extra": true}),
        );
        assert!(refused.is_err());
    }
}
